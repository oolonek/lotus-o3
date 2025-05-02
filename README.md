# LOTUS-O3 (LOTUS Oxyde)

This Rust crate provides a command-line tool to process CSV files containing chemical occurrence data (chemical name, SMILES, taxon name, reference DOI) and prepare it for addition to Wikidata.

## Features

*   **CSV Loading & Validation:** Loads data from a CSV file, validates required headers (`chemical_entity_name`, `chemical_entity_smiles`, `taxon_name`, `reference_doi`), and ensures these columns are not empty.
*   **Chemical Data Enrichment:** Uses the public Chemoinformatics API (`https://api.naturalproducts.net`) to enrich the input SMILES with:
    *   Canonical SMILES
    *   Isomeric SMILES
    *   InChI
    *   InChIKey
    *   Molecular Formula
*   **Wikidata Checks:** Queries the Wikidata SPARQL endpoint to check if:
    *   The chemical entity already exists (using InChIKey).
    *   The taxon already exists (using its name).
    *   The reference publication already exists (using its DOI).
    *   The specific occurrence (chemical found in taxon, stated in reference) already exists.
*   **QuickStatements Generation:** Generates a file compatible with Wikidata's QuickStatements V1 tool. This file includes commands to:
    *   Create new chemical items if they don't exist (as 'type of chemical entity' - Q113145171), including properties like SMILES, InChI, InChIKey, formula, label, and description.
    *   Add 'found in taxon' (P703) statements to chemical items, referencing the publication (using 'stated in' - S248).
*   **Logging:** Provides informative logs during processing.
*   **Error Handling:** Logs errors encountered during processing (CSV issues, API errors, Wikidata query failures) and continues with the next record.
*   **Summary Report:** Prints a summary of processed records and errors at the end.

## Usage

1.  **Build the Crate:**
    ```bash
    # Ensure Rust and Cargo are installed (https://rustup.rs/)
    cargo build --release
    ```
    The executable will be located at `./target/release/lotus-o3`.

2.  **Prepare Input CSV:**
    Create a CSV file with the following required columns:
    *   `chemical_entity_name`: The name of the chemical compound.
    *   `chemical_entity_smiles`: A SMILES representation of the compound.
    *   `taxon_name`: The name of the taxon the compound is found in.
    *   `reference_doi`: The DOI of the paper describing the occurrence.

    Example `input.csv`:
    ```csv
    chemical_entity_name,chemical_entity_smiles,taxon_name,reference_doi
    Caffeine,CN1C=NC2=C1C(=O)N(C(=O)N2C)C,Coffea arabica,10.1007/s00217-005-0029-5
    Theobromine,Cn1cnc2c1c(=O)nc(n2)C,Theobroma cacao,10.1016/j.jep.2008.06.008
    NonExistentCompound,CCCCCCCCCCC,Fake Taxon,10.9999/fake.doi
    ```

3.  **Run the Importer:**
    The primary mode is generating a QuickStatements file.

    ```bash
    ./target/release/lotus-o3 -i input.csv -o output.qs
    ```
    *   `-i, --input-file <FILE>`: Path to the input CSV file (required).
    *   `-o, --output-file <FILE>`: Path to the output QuickStatements file (required for QuickStatements mode).
    *   `-m, --mode <MODE>`: Output mode. Options: `qs` (default), `direct` (not implemented). Use `qs`.

4.  **Upload to QuickStatements:**
    *   Go to the [QuickStatements tool](https://quickstatements.toolforge.org/).
    *   Log in.
    *   Click "New batch".
    *   Paste the contents of the generated `output.qs` file into the text area.
    *   Click "Import V1 commands".
    *   Review the commands and click "Run".

## Development Notes

*   **Dependencies:** Uses `csv`, `serde`, `reqwest`, `tokio`, `clap`, `log`, `env_logger`, `thiserror`, `serde_json`.
*   **API Interaction:** Interacts with `api.naturalproducts.net` for enrichment and `query.wikidata.org` for checks.
*   **Wikidata Edits:** Currently only supports generating QuickStatements. Direct editing via the API is not implemented due to authentication complexities.
*   **Error Handling:** Aims to be robust by logging errors and continuing processing.
*   **Testing:** Includes unit tests for CSV parsing, CLI parsing, and QuickStatements generation. Integration tests hitting live APIs/Wikidata are marked `#[ignore]` and should be run cautiously.

## Future Improvements

*   Implement direct Wikidata editing with proper authentication.
*   Add calculation for mass (P2067) using a Rust cheminformatics library.
*   Improve taxon name matching (handle ambiguity, case-insensitivity).
*   Add more detailed logging levels and configuration.
*   Implement mocking for API and SPARQL endpoints for more reliable testing.
*   Provide more detailed summary statistics.

