pub mod cli;
pub mod csv_handler;
pub mod enrichment;
pub mod error;
pub mod reference;
pub mod wikidata;

use clap::Parser;
use cli::{Cli, OutputMode};
use csv::WriterBuilder;
use csv_handler::{ColumnConfig, load_and_validate_csv};
use enrichment::{EnrichedData, enrich_record};
use error::{CrateError, Result};
use indicatif::{ProgressBar, ProgressStyle};
use log::{error, info, warn};
use reqwest::Client;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;
use urlencoding::encode;
use wikidata::checker::{WikidataInfo, check_wikidata};
use wikidata::writer::generate_quickstatements;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logger
    // env_logger::init();
    env_logger::Builder::from_default_env()
        .format_target(false) // Option: to hide the module path prefix for cleaner logs
        .format_timestamp_secs() // Option: to have simpler timestamps
        .filter_level(log::LevelFilter::Info) // Set a default filter level (e.g., Info)
        .try_init() // Use try_init() to handle potential errors if logger is already set
        .expect("Failed to initialize logger"); // Or handle the error more gracefully

    // Parse CLI arguments
    let cli = Cli::parse();
    info!("Starting Wikidata Importer...");
    info!("Input file: {:?}", cli.input_file);
    info!("Output mode: {:?}", cli.mode);
    if let Some(output_file) = &cli.output_file {
        info!("Output file: {:?}", output_file);
    }

    let start_time = Instant::now();

    // 1. Load and Validate CSV
    info!("Loading and validating CSV...");
    let column_config = ColumnConfig {
        chemical_name: cli.column_chemical_name.clone(),
        structure: cli.column_structure.clone(),
        taxon: cli.column_taxon.clone(),
        doi: cli.column_doi.clone(),
    };

    let input_records = match load_and_validate_csv(&cli.input_file, &column_config) {
        Ok(records) => {
            info!(
                "Successfully loaded and validated {} records.",
                records.len()
            );
            records
        }
        Err(e) => {
            error!("Failed to load or validate CSV: {}", e);
            return Err(e);
        }
    };

    if input_records.is_empty() {
        info!("Input CSV is empty or contains no valid records. Exiting.");
        return Ok(());
    }

    // 2. Process Records (Enrichment & Wikidata Check)
    info!("Processing records (enrichment and Wikidata checks)...");
    // Explicitly map the reqwest::Error from client building
    let client = Client::builder()
        .user_agent(wikidata::checker::USER_AGENT) // Use the defined user agent
        .build()
        .map_err(CrateError::ApiRequestError)?;

    let mut processed_data = Vec::new();
    let mut errors_count = 0;
    let mut error_details: Vec<String> = Vec::new();

    // Initialize the progress bar
    let num_records = input_records.len() as u64;
    let pb = ProgressBar::new(num_records);
    pb.set_style(ProgressStyle::default_bar()
        .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({percent}%) {msg}")
        .expect("Failed to set progress bar style") // Added expect for error handling
        .progress_chars("##-"));

    for (index, record) in input_records.into_iter().enumerate() {
        let row_num = index + 2; // CSV row number (1-based + header)
        let smiles = record.chemical_entity_smiles.clone(); // Clone for error reporting
        let chemical_entity_name = record.chemical_entity_name.clone(); // Clone for error reporting

        // Update progress bar message (optional)
        pb.set_message(format!("Processing: {} ({})", chemical_entity_name, smiles));

        match enrich_record(record, &client).await {
            Ok(enriched) => {
                let inchikey = enriched
                    .inchikey
                    .clone()
                    .unwrap_or_else(|| "N/A".to_string());
                match check_wikidata(&enriched, &client).await {
                    Ok(wikidata_info) => {
                        processed_data.push((enriched, wikidata_info));
                    }
                    Err(e) => {
                        let error_message = format!(
                            "Row {}: Wikidata check failed for InChIKey {}: {}",
                            row_num, inchikey, e
                        );
                        pb.println(format!(
                            "Error (Wikidata check) for row {}: {} - {}",
                            row_num, inchikey, e
                        )); // For progress bar
                        error!("{}", error_message); // Your existing log
                        error_details.push(error_message);
                        errors_count += 1;
                    }
                }
            }
            Err(e) => {
                let error_message = format!(
                    "Row {}: Enrichment failed for SMILES {}: {}",
                    row_num, smiles, e
                );
                pb.println(format!(
                    "Error (Enrichment) for row {}: {} - {}",
                    row_num, smiles, e
                )); // For progress bar
                error!("{}", error_message); // Your existing log
                error_details.push(error_message); // <<< ADD THIS LINE
                errors_count += 1;
            }
        }
        pb.inc(1); // Increment the progress bar
    }

    // Finish the progress bar
    pb.finish_with_message("Record processing complete.");

    info!(
        "Finished processing records. {} successful, {} errors.",
        processed_data.len(),
        errors_count
    );

    let record_reports = build_record_reports(&processed_data);
    let chemical_creations = record_reports.iter().filter(|r| r.create_chemical).count();
    let reference_creations = record_reports.iter().filter(|r| r.create_reference).count();
    let occurrence_creations = record_reports
        .iter()
        .filter(|r| r.create_occurrence)
        .count();
    let deferred_occurrences = record_reports
        .iter()
        .filter(|r| r.occurrence_waiting_on_reference)
        .count();
    let unresolved_taxa = record_reports
        .iter()
        .filter(|r| r.taxon_qid.is_none())
        .count();
    let problematic_records = record_reports
        .iter()
        .filter(|r| !r.issues.is_empty())
        .count();

    // 3. Output Generation
    let mut status_report_path: Option<PathBuf> = None;
    let mut qs_artifacts: Option<QuickstatementArtifacts> = None;
    let mut quickstatements_file: Option<PathBuf> = None;
    match cli.mode {
        OutputMode::QuickStatements => {
            let output_path = cli
                .output_file
                .expect("Output file path is required for QS mode");
            info!("Generating QuickStatements file: {:?}...", output_path);
            match File::create(&output_path) {
                Ok(file) => {
                    let mut writer = BufWriter::new(file);
                    if let Err(e) = generate_quickstatements(&processed_data, &mut writer) {
                        error!("Failed to generate QuickStatements: {}", e);
                        return Err(e);
                    }
                    writer.flush().map_err(CrateError::IoError)?;
                    info!(
                        "Successfully generated QuickStatements file at: {:?}",
                        output_path
                    );
                    let artifacts = handle_quickstatement_artifacts(&output_path, &record_reports)?;
                    status_report_path = Some(artifacts.status_report.clone());
                    qs_artifacts = Some(artifacts);
                    quickstatements_file = Some(output_path);
                }
                Err(e) => {
                    error!("Failed to create output file {:?}: {}", output_path, e);
                    return Err(CrateError::IoError(e));
                }
            }
        }
        OutputMode::DirectPush => {
            warn!("Direct push mode is not yet implemented.");
            // Placeholder for future implementation
            // if let Err(e) = wikidata::writer::push_to_wikidata(&processed_data, &client).await {
            //     error!("Failed to push data directly to Wikidata: {}", e);
            //     return Err(e);
            // }
        }
    }

    let duration = start_time.elapsed();
    info!("Total execution time: {:.2?}", duration);

    // Basic Summary Report
    println!("\n--- Summary Report ---");
    println!(
        "Total CSV records read (from initial validation): {}",
        processed_data.len() + errors_count
    );
    println!(
        "Successfully processed (passed enrichment and Wikidata checks): {}",
        processed_data.len()
    );
    println!("Chemical items queued for creation: {}", chemical_creations);
    println!(
        "Reference items queued for creation: {}",
        reference_creations
    );
    println!("Occurrence statements queued: {}", occurrence_creations);
    if deferred_occurrences > 0 {
        println!(
            "Occurrence statements waiting on new references: {}",
            deferred_occurrences
        );
        println!(
            "  QuickStatements cannot cite items created earlier in the same batch; \
rerun this tool after the reference batch finishes to emit those occurrences."
        );
    }
    if unresolved_taxa > 0 {
        println!(
            "Records without a Wikidata taxon (not auto-created): {}",
            unresolved_taxa
        );
        println!("  Taxonomic name resolution/creation is not yet supported.");
    }
    if problematic_records > 0 {
        println!(
            "Records requiring manual review: {} (see status report for details)",
            problematic_records
        );
    }
    println!("Errors encountered during processing: {}", errors_count);
    if !error_details.is_empty() {
        println!("\n--- Detailed Errors ---");
        for detail in error_details {
            println!("- {}", detail);
        }
    }
    if let Some(report_path) = status_report_path.clone() {
        println!(
            "Per-record status report saved to: {}",
            report_path.display()
        );
    }
    println!("\n--- Next actions ---");
    if let Some(qs_path) = &quickstatements_file {
        if let Some(artifacts) = &qs_artifacts {
            if let Some(url_file) = &artifacts.qs_url_file {
                println!(
                    "- Submit {} via QuickStatements (https://quickstatements.toolforge.org/#/batch). Alternatively, a ready-to-run link also saved in {}).",
                    qs_path.display(),
                    url_file.display()
                );
            } else {
                println!("- Submit {} via QuickStatements.", qs_path.display());
            }
        } else {
            println!("- Submit {} via QuickStatements.", qs_path.display());
        }
    } else {
        println!("- No QuickStatements batch generated in this run; nothing to upload.");
    }
    if deferred_occurrences > 0 {
        println!(
            "- After this batch finishes, rerun lotus-o3 to emit the {} deferred occurrence statement(s).",
            deferred_occurrences
        );
    } else if quickstatements_file.is_some() {
        println!("- Once the QuickStatements run completes, no second pass is required.");
    }
    if let Some(report_path) = &status_report_path {
        println!(
            "- Review per-record results in {} for any flagged issues.",
            report_path.display()
        );
    }
    if problematic_records > 0 {
        println!(
            "- Address the {} record(s) requiring manual review noted above.",
            problematic_records
        );
    }
    if unresolved_taxa > 0 {
        println!(
            "- Resolve the {} missing taxon QID(s) manually on Wikidata before rerunning.",
            unresolved_taxa
        );
    }
    println!("Execution time: {:.2?}", duration);

    Ok(())
}

fn handle_quickstatement_artifacts(
    output_path: &Path,
    records: &[RecordReport],
) -> Result<QuickstatementArtifacts> {
    let report_path = build_report_path(output_path);
    write_status_report(records, &report_path)?;
    println!("Per-record status saved to {}", report_path.display());

    let qs_content = fs::read_to_string(output_path)?;
    let mut qs_url_file = None;
    if qs_content.trim().is_empty() {
        println!(
            "\nQuickStatements file {} is empty; nothing to submit.",
            output_path.display()
        );
    } else {
        println!(
            "\nQuickStatements commands saved to {}.",
            output_path.display()
        );
        println!(
            "Submit them via https://quickstatements.toolforge.org/ by pasting the file contents or opening this ready-to-run link (OAuth required):"
        );
        let qs_url = quickstatements_link(&qs_content);
        println!("{}", qs_url);
        let url_path = build_qs_link_path(output_path);
        fs::write(&url_path, format!("{}\n", qs_url))?;
        println!("QuickStatements URL saved to {}", url_path.display());
        qs_url_file = Some(url_path);
    }

    Ok(QuickstatementArtifacts {
        status_report: report_path,
        qs_url_file,
    })
}

fn build_record_reports(records: &[(EnrichedData, WikidataInfo)]) -> Vec<RecordReport> {
    records
        .iter()
        .map(|(data, info)| {
            let create_chemical = info.chemical_qid.is_none();
            let create_reference =
                info.reference_qid.is_none() && info.reference_metadata.is_some();
            let chemical_available = info.chemical_qid.is_some() || create_chemical;
            let reference_qid_available = info.reference_qid.is_some();
            let reference_available = reference_qid_available || create_reference;
            let taxon_available = info.taxon_qid.is_some();
            let create_occurrence =
                !info.occurrence_exists
                    && chemical_available
                    && reference_qid_available
                    && taxon_available;
            let waiting_on_reference = !info.occurrence_exists
                && taxon_available
                && chemical_available
                && !reference_qid_available
                && info.reference_metadata.is_some();

            let mut issues = Vec::new();
            if info.taxon_qid.is_none() {
                issues.push(
                    "Taxon entity not found in Wikidata; taxonomic name resolution is not implemented."
                        .to_string(),
                );
            }
            if info.reference_qid.is_none() && info.reference_metadata.is_none() {
                issues.push(
                    "DOI missing in Wikidata and Crossref lookup failed; reference must be curated manually."
                        .to_string(),
                );
            }
            if waiting_on_reference {
                issues.push(
                    "Occurrence deferred until the new reference item has a QID; rerun the importer after this batch finishes in QuickStatements."
                        .to_string(),
                );
            } else if !info.occurrence_exists && !create_occurrence && taxon_available {
                if !reference_available {
                    issues.push("Missing reference metadata prevents occurrence creation.".to_string());
                } else if !chemical_available {
                    issues.push("Missing chemical data prevents occurrence creation.".to_string());
                }
            }

            RecordReport {
                chemical_entity_name: data.chemical_entity_name.clone(),
                chemical_entity_smiles: data.sanitized_smiles.clone(),
                taxon_name: data.taxon_name.clone(),
                reference_doi: data.reference_doi.clone(),
                chemical_qid: info.chemical_qid.clone(),
                taxon_qid: info.taxon_qid.clone(),
                reference_qid: info.reference_qid.clone(),
                create_chemical,
                create_reference,
                create_occurrence,
                occurrence_waiting_on_reference: waiting_on_reference,
                issues,
            }
        })
        .collect()
}

fn write_status_report(rows: &[RecordReport], path: &Path) -> Result<()> {
    let mut writer = WriterBuilder::new().delimiter(b'\t').from_path(path)?;
    writer.write_record([
        "chemical_entity_name",
        "chemical_entity_smiles",
        "taxon_name",
        "reference_doi",
        "chemical_qid",
        "taxon_qid",
        "reference_qid",
        "create_chemical",
        "create_reference",
        "create_occurrence",
        "occurrence_waiting_on_reference",
        "issues",
    ])?;

    for row in rows {
        let issues_text = if row.issues.is_empty() {
            "".to_string()
        } else {
            row.issues.join("; ")
        };
        writer.write_record([
            row.chemical_entity_name.as_str(),
            row.chemical_entity_smiles.as_str(),
            row.taxon_name.as_str(),
            row.reference_doi.as_str(),
            row.chemical_qid.as_deref().unwrap_or(""),
            row.taxon_qid.as_deref().unwrap_or(""),
            row.reference_qid.as_deref().unwrap_or(""),
            bool_to_label(row.create_chemical),
            bool_to_label(row.create_reference),
            bool_to_label(row.create_occurrence),
            bool_to_label(row.occurrence_waiting_on_reference),
            issues_text.as_str(),
        ])?;
    }

    writer.flush()?;
    Ok(())
}

fn build_report_path(output_path: &Path) -> PathBuf {
    let stem = output_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("status");
    let file_name = format!("{}_status.tsv", stem);
    output_path.with_file_name(file_name)
}

fn build_qs_link_path(output_path: &Path) -> PathBuf {
    let stem = output_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("quickstatements");
    let file_name = format!("{}_qs_url.txt", stem);
    output_path.with_file_name(file_name)
}

fn quickstatements_link(contents: &str) -> String {
    let normalized = contents.replace('\r', "");
    let replaced = normalized.replace('\t', "|").replace('\n', "||");
    format!(
        "https://quickstatements.toolforge.org/#/v1={}",
        encode(&replaced)
    )
}

fn bool_to_label(flag: bool) -> &'static str {
    if flag { "yes" } else { "no" }
}

struct RecordReport {
    chemical_entity_name: String,
    chemical_entity_smiles: String,
    taxon_name: String,
    reference_doi: String,
    chemical_qid: Option<String>,
    taxon_qid: Option<String>,
    reference_qid: Option<String>,
    create_chemical: bool,
    create_reference: bool,
    create_occurrence: bool,
    occurrence_waiting_on_reference: bool,
    issues: Vec<String>,
}

struct QuickstatementArtifacts {
    status_report: PathBuf,
    qs_url_file: Option<PathBuf>,
}
