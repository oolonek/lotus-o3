use crate::enrichment::EnrichedData;
use crate::error::{CrateError, Result};
use crate::wikidata::checker::WikidataInfo;
use std::io::Write;

// Generates a QuickStatements V1 command string for a list of records.
pub fn generate_quickstatements(
    records: &[(EnrichedData, WikidataInfo)],
    writer: &mut dyn Write,
) -> Result<()> {
    let mut temp_qid_counter = 0;

    for (data, info) in records {
        let mut commands = Vec::new();
        let mut current_chemical_qid = info.chemical_qid.clone();

        // 1. Create Chemical Item if it doesn't exist
        if info.chemical_qid.is_none() {
            temp_qid_counter += 1;
            let temp_qid = format!("CREATE_{}", temp_qid_counter);
            commands.push("CREATE".to_string());
            current_chemical_qid = Some(temp_qid.clone()); // Use temporary ID for subsequent commands

            // Add Label (L), Description (D), Alias (A)
            commands.push(format!("LAST\tLen\t\"{}\"", data.chemical_entity_name));
            // Description based on knowledge module preference
            commands.push("LAST\tDen\t\"type of chemical entity\"".to_string());
            // Potentially add aliases if needed

            // Add P31 (instance of) -> Q113145171 (type of chemical entity)
            commands.push("LAST\tP31\tQ113145171".to_string());

            // Add Chemical Properties
            if let Some(smiles) = &data.canonical_smiles {
                commands.push(format!("LAST\tP233\t\"{}\"", smiles));
            }
            if let Some(smiles) = &data.isomeric_smiles {
                commands.push(format!("LAST\tP2017\t\"{}\"", smiles));
            }
            if let Some(inchi) = &data.inchi {
                commands.push(format!("LAST\tP234\t\"{}\"", inchi));
            }
            if let Some(inchikey) = &data.inchikey {
                commands.push(format!("LAST\tP235\t\"{}\"", inchikey));
            }
            if let Some(formula) = &data.molecular_formula {
                commands.push(format!("LAST\tP274\t\"{}\"", formula));
            }
            
            // Add occurrence statement with temporary ID
            commands.push(format!(
                "LAST\tP703\t{}\tS248\t{}",
                info.taxon_qid.as_ref().unwrap(), info.reference_qid.as_ref().unwrap()
            ));

            // TODO: Add P2067 (mass) calculation and statement with qualifiers P887=Q113907573
            // Requires a cheminformatics library capable of calculating mass from SMILES/formula.
            // Skipping for simplicity for now.
        }

        // 2. Add Occurrence Statement if it doesn't exist and all QIDs are present
        if !info.occurrence_exists && info.chemical_qid.is_some() {
            match (&current_chemical_qid, &info.taxon_qid, &info.reference_qid) {
                (Some(chem_qid), Some(tax_qid), Some(ref_qid)) => {
                    // Add P703 (found in taxon) statement with S248 (stated in) reference
                    commands.push(format!(
                        "{}\tP703\t{}\tS248\t{}",
                        chem_qid, tax_qid, ref_qid
                    ));
                    // Log or report that occurrence was added
                    eprintln!(
                        "Added occurrence for {} - Chem: {:?}, Taxon: {:?}, Ref: {:?}",
                        data.inchikey.as_deref().unwrap_or("N/A"),
                        chem_qid,
                        tax_qid,
                        ref_qid
                    );
                }
                _ => {
                    // Log or report that occurrence couldn't be added due to missing QIDs
                    // This case should ideally be handled earlier or logged separately.
                    // For QuickStatements generation, we just skip the command.
                    eprintln!(
                        "Skipping occurrence for {} - missing QID (Chem: {:?}, Taxon: {:?}, Ref: {:?})",
                        data.inchikey.as_deref().unwrap_or("N/A"),
                        current_chemical_qid,
                        info.taxon_qid,
                        info.reference_qid
                    );
                    // Optionally return an error or specific status
                    // return Err(CrateError::MissingQidForOccurrence { ... });
                }
            }
        }

        // Write commands for this record to the writer
        for command in commands {
            writeln!(writer, "{}", command).map_err(|e| CrateError::IoError(e))?;
        }
    }

    Ok(())
}

// --- Direct Wikidata Edit (Placeholder/Future Implementation) ---
// This section would require handling authentication (OAuth or bot credentials)
// and using a Wikidata edit API client (e.g., `wikidata` crate or custom `reqwest` calls).

// pub async fn push_to_wikidata(
//     records: &[(EnrichedData, WikidataInfo)],
//     // auth_token: &str, // Or other auth mechanism
//     client: &reqwest::Client,
// ) -> Result<()> {
//     // ... implementation for direct edits ...
//     // - Create items if needed
//     // - Add statements (P31, chemical props, P703)
//     // - Handle edit conflicts, rate limits, etc.
//     Err(CrateError::WikidataWriteError("Direct push not yet implemented".to_string()))
// }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enrichment::EnrichedData;
    use crate::wikidata::checker::WikidataInfo;
    use std::io::Cursor;

    fn create_test_data(
        chem_qid: Option<&str>,
        tax_qid: Option<&str>,
        ref_qid: Option<&str>,
        occurrence_exists: bool,
    ) -> (EnrichedData, WikidataInfo) {
        (
            EnrichedData {
                chemical_entity_name: "TestChem".to_string(),
                input_smiles: "C".to_string(),
                sanitized_smiles: "C".to_string(),
                taxon_name: "TestTaxon".to_string(),
                reference_doi: "10.1/test".to_string(),
                canonical_smiles: Some("C".to_string()),
                isomeric_smiles: None,
                inchi: Some("InChI=1S/CH4/h1H4".to_string()),
                inchikey: Some("VNWKTOKETHGBQD-UHFFFAOYSA-N".to_string()),
                molecular_formula: Some("CH4".to_string()),
                other_descriptors: None,
            },
            WikidataInfo {
                chemical_qid: chem_qid.map(String::from),
                taxon_qid: tax_qid.map(String::from),
                reference_qid: ref_qid.map(String::from),
                occurrence_exists,
            },
        )
    }

    #[test]
    fn test_generate_qs_create_item_and_occurrence() {
        let records = vec![create_test_data(None, Some("Q2"), Some("Q3"), false)];
        let mut buffer = Cursor::new(Vec::new());
        generate_quickstatements(&records, &mut buffer).unwrap();

        let output = String::from_utf8(buffer.into_inner()).unwrap();
        let lines: Vec<&str> = output.trim().split('\n').collect();
        println!("Generated QuickStatements:\n{}", output);

        assert!(lines.contains(&"CREATE"));
        // Use raw strings r#"..."# to avoid issues with escapes
        assert!(lines.contains(&r#"LAST	Len	"TestChem""#)); 
        assert!(lines.contains(&r#"LAST	Den	"type of chemical entity""#));
        assert!(lines.contains(&r#"LAST	P31	Q113145171"#));
        assert!(lines.contains(&r#"LAST	P233	"C""#)); // Canonical SMILES
        assert!(lines.contains(&r#"LAST	P234	"InChI=1S/CH4/h1H4""#)); // InChI
        assert!(lines.contains(&r#"LAST	P235	"VNWKTOKETHGBQD-UHFFFAOYSA-N""#)); // InChIKey
        assert!(lines.contains(&r#"LAST	P274	"CH4""#)); // Formula
        // Check occurrence statement using the temporary ID
        assert!(lines.contains(&r#"LAST	P703	Q2	S248	Q3"#));
    }

    #[test]
    fn test_generate_qs_add_occurrence_only() {
        let records = vec![create_test_data(Some("Q1"), Some("Q2"), Some("Q3"), false)];
        let mut buffer = Cursor::new(Vec::new());
        generate_quickstatements(&records, &mut buffer).unwrap();

        let output = String::from_utf8(buffer.into_inner()).unwrap();
        let lines: Vec<&str> = output.trim().split('\n').collect();

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0], r#"Q1	P703	Q2	S248	Q3"#);
    }

    #[test]
    fn test_generate_qs_skip_existing_occurrence() {
        let records = vec![create_test_data(Some("Q1"), Some("Q2"), Some("Q3"), true)];
        let mut buffer = Cursor::new(Vec::new());
        generate_quickstatements(&records, &mut buffer).unwrap();

        let output = String::from_utf8(buffer.into_inner()).unwrap();
        assert!(output.trim().is_empty());
    }

    #[test]
    fn test_generate_qs_skip_missing_taxon_qid() {
        // Chemical exists, Taxon doesn't, Ref exists, Occurrence doesn't
        let records = vec![create_test_data(Some("Q1"), None, Some("Q3"), false)];
        let mut buffer = Cursor::new(Vec::new());
        generate_quickstatements(&records, &mut buffer).unwrap();

        let output = String::from_utf8(buffer.into_inner()).unwrap();
        // No occurrence command should be generated
        assert!(output.trim().is_empty());
        // Check stderr/log for the skip message (cannot check directly here)
    }
     #[test]
    fn test_generate_qs_multiple_records() {
        let records = vec![
            create_test_data(None, Some("Q2"), Some("Q3"), false), // Create Chem1, Add Occ1
            create_test_data(Some("Q4"), Some("Q5"), Some("Q6"), false), // Add Occ2
            create_test_data(Some("Q7"), Some("Q8"), Some("Q9"), true), // Skip Occ3
        ];
        let mut buffer = Cursor::new(Vec::new());
        generate_quickstatements(&records, &mut buffer).unwrap();

        let output = String::from_utf8(buffer.into_inner()).unwrap();
        let lines: Vec<&str> = output.trim().split('\n').collect();
        println!("Generated QuickStatements:\n{}", output);

        // Check commands for first record (creation + occurrence)
        assert!(lines.contains(&"CREATE"));
        assert!(lines.contains(&r#"LAST	Len	"TestChem""#));
        assert!(lines.contains(&r#"LAST	P703	Q2	S248	Q3"#));
        // Check command for second record (occurrence only)
        assert!(lines.contains(&r#"Q4	P703	Q5	S248	Q6"#));
        // Check that nothing was generated for the third record
        assert!(!lines.iter().any(|&l| l.starts_with("Q7\t")));
        assert!(lines.len() > 2); // Ensure multiple commands were generated
    }
}

