pub mod error;
pub mod csv_handler;
pub mod enrichment;
pub mod wikidata;
pub mod cli;

use clap::Parser;
use cli::{Cli, OutputMode};
use csv_handler::load_and_validate_csv;
use enrichment::enrich_record;
use error::{CrateError, Result};
use log::{error, info, warn};
use reqwest::Client;
use std::fs::File;
use std::io::BufWriter;
use std::time::Instant;
use wikidata::checker::check_wikidata;
use wikidata::writer::generate_quickstatements;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logger
    env_logger::init();

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
    let input_records = match load_and_validate_csv(&cli.input_file) {
        Ok(records) => {
            info!("Successfully loaded and validated {} records.", records.len());
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

    for (index, record) in input_records.into_iter().enumerate() {
        let row_num = index + 2; // CSV row number (1-based + header)
        let smiles = record.chemical_entity_smiles.clone(); // Clone for error reporting
        
        match enrich_record(record, &client).await {
            Ok(enriched) => {
                let inchikey = enriched.inchikey.clone().unwrap_or_else(|| "N/A".to_string());
                match check_wikidata(&enriched, &client).await {
                    Ok(wikidata_info) => {
                        processed_data.push((enriched, wikidata_info));
                    }
                    Err(e) => {
                        error!("Row {}: Wikidata check failed for InChIKey {}: {}", row_num, inchikey, e);
                        errors_count += 1;
                    }
                }
            }
            Err(e) => {
                error!("Row {}: Enrichment failed for SMILES {}: {}", row_num, smiles, e);
                errors_count += 1;
            }
        }
    }

    info!(
        "Finished processing records. {} successful, {} errors.",
        processed_data.len(),
        errors_count
    );

    // 3. Output Generation
    match cli.mode {
        OutputMode::QuickStatements => {
            let output_path = cli.output_file.expect("Output file path is required for QS mode");
            info!("Generating QuickStatements file: {:?}...", output_path);
            match File::create(&output_path) {
                Ok(file) => {
                    let mut writer = BufWriter::new(file);
                    if let Err(e) = generate_quickstatements(&processed_data, &mut writer) {
                        error!("Failed to generate QuickStatements: {}", e);
                        return Err(e);
                    }
                    info!("Successfully generated QuickStatements file.");
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
    println!("Total records processed: {}", processed_data.len() + errors_count);
    println!("Successfully processed: {}", processed_data.len());
    println!("Errors encountered: {}", errors_count);
    // TODO: Add more detailed stats (new chemicals, new occurrences, etc.)

    Ok(())
}

