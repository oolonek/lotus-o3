# LOTUS-O3 (LOTUS Oxyde)

This Rust crate provides a command-line tool to process CSV files containing chemical occurrence data (chemical name, SMILES, taxon name, reference DOI) and prepare it for addition to Wikidata.

## Features

*   **CSV Loading & Validation:** Loads data from a CSV file and validates the required columns. If your headers differ, use `--column-chemical-name`, `--column-structure`, `--column-taxon`, or `--column-doi` to remap them. Missing-column errors now list all required headers and the CLI overrides.
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
    *   Create missing reference items from Crossref metadata (including volume, issue, monolingual title, authors).
*   **User Guidance:** Each run emits a per-record TSV status report, a ready-to-run QuickStatements link saved in `<output_stem>_qs_url.txt`, and a “Next actions” block explaining whether a second QS run is required.
*   **Caching:** Crossref lookups and reference DOIs are cached per run, so repeated DOIs are fetched only once.
*   **Logging & Summary:** Verbose logs plus a summary report detailing successes, manual-review counts, deferred occurrences, and unresolved taxa.

## Usage

1.  **Build the Crate:**
    ```bash
    # Ensure Rust and Cargo are installed (https://rustup.rs/)
    cargo build --release
    ```
    The executable will be located at `./target/release/lotus-o3`.

2.  **Prepare Input CSV:**
    Create a CSV file with the following required columns (or supply their aliases via CLI flags):
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
    # Headers already match the defaults, so no overrides are needed:
    ./target/release/lotus-o3 -i input.csv -o output.qs

    # If your CSV uses different column names, supply the overrides:
    ./target/release/lotus-o3 \
      -i input.csv \
      -o output.qs \
      --column-chemical-name name \
      --column-structure smiles \
      --column-taxon taxon \
      --column-doi doi
    ```
*   `-i, --input-file <FILE>`: Path to the input CSV file (required).
    *   `-o, --output-file <FILE>`: Path to the output QuickStatements file (required in QS mode).
    *   `--column-*`: Optional overrides for the header names described above.
    *   `-m, --mode <MODE>`: Output mode. Options: `qs` (default), `direct` (not implemented). Use `qs`.

4.  **Upload to QuickStatements:**
    *   Go to the [QuickStatements tool](https://quickstatements.toolforge.org/).
    *   Log in.
    *   Click "New batch".
    *   Either paste the contents of the generated `output.qs` file or open the ready-to-run URL saved in `<output_stem>_qs_url.txt`.
    *   Click "Import V1 commands".
    *   Review the commands and click "Run".

    Each run also emits:
    *   `output_status.tsv` — a per-record TSV summarizing which chemicals/references/occurrences will be created.
    *   `<output_stem>_qs_url.txt` — the ready-to-run QuickStatements link for the batch.
    *   A console “Next actions” block telling you whether a follow-up run is required (e.g., after reference items finish creating).

## Development Notes

*   **Dependencies:** Uses `csv`, `serde`, `reqwest`, `tokio`, `clap`, `log`, `env_logger`, `thiserror`, `serde_json`, `once_cell`, `indicatif`.
*   **API Interaction:** Interacts with `api.naturalproducts.net` for enrichment and `query.wikidata.org` for checks.
*   **Wikidata Edits:** Currently only supports generating QuickStatements. Direct editing via the API is not implemented due to authentication complexities.
*   **Error Handling:** Aims to be robust by logging errors and continuing processing.
*   **Testing:** Includes unit tests for CSV parsing, CLI parsing, enrichment, and QuickStatements generation. Integration tests hitting live APIs/Wikidata are marked `#[ignore]` and should be run cautiously (`cargo test`).
*   **Documentation:** To build and browse the API docs (with module-level descriptions added), run `cargo doc --open`.

## Installing / Running from PATH

`lotus-o3` is not on crates.io yet. To have `lotus-o3` on your PATH:

```bash
git clone https://github.com/<your-org>/lotus-o3.git
cd lotus-o3
cargo install --path .
# or link the release build manually:
ln -s "$(pwd)/target/release/lotus-o3" ~/bin/lotus-o3
```

After `cargo install --path .`, Cargo places the binary in `~/.cargo/bin`, so running `lotus-o3 -i ... -o ...` works from anywhere.

## Future Improvements

*   Implement direct Wikidata editing with proper authentication.
*   Add calculation for mass (P2067) using a Rust cheminformatics library.
*   Improve taxon name matching (handle ambiguity, case-insensitivity).
*   Add more detailed logging levels and configuration.
*   Implement mocking for API and SPARQL endpoints for more reliable testing.
*   Provide more detailed summary statistics.
