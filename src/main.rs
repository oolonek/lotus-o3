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
use reqwest::dns::Name;
use reqwest::Client;
use std::fs::File;
use std::io::BufWriter;
use std::time::Instant;
use wikidata::checker::check_wikidata;
use wikidata::writer::generate_quickstatements;
use indicatif::{ProgressBar, ProgressStyle};

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
                let inchikey = enriched.inchikey.clone().unwrap_or_else(|| "N/A".to_string());
                match check_wikidata(&enriched, &client).await {
                    Ok(wikidata_info) => {
                        processed_data.push((enriched, wikidata_info));
                    }
                    Err(e) => {
                        let error_message = format!("Row {}: Wikidata check failed for InChIKey {}: {}", row_num, inchikey, e);
                        pb.println(format!("Error (Wikidata check) for row {}: {} - {}", row_num, inchikey, e)); // For progress bar
                        error!("{}", error_message); // Your existing log
                        error_details.push(error_message);
                        errors_count += 1;
                    }
                }
            }
            Err(e) => {
                let error_message = format!("Row {}: Enrichment failed for SMILES {}: {}", row_num, smiles, e);
                pb.println(format!("Error (Enrichment) for row {}: {} - {}", row_num, smiles, e)); // For progress bar
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


    // Calculate additional statistics
    let mut new_chemicals_to_create = 0;
    let mut new_occurrences_to_create = 0;

    for (enriched_data_item, wikidata_info_item) in &processed_data {
        // Check for new chemicals
        if wikidata_info_item.chemical_qid.is_none() {
            new_chemicals_to_create += 1;
        }

        // Check for new occurrences to be created
        // An occurrence will be created if it doesn't exist AND
        // (the chemical QID exists OR it's a new chemical being created) AND
        // the taxon QID exists AND the reference QID exists.
        if !wikidata_info_item.occurrence_exists {
            let chemical_will_have_qid = wikidata_info_item.chemical_qid.is_some() || 
                                        (wikidata_info_item.chemical_qid.is_none() && enriched_data_item.inchikey.is_some()); // Assuming new chem needs at least an InChIKey to be valid
            
            if chemical_will_have_qid && 
            wikidata_info_item.taxon_qid.is_some() && 
            wikidata_info_item.reference_qid.is_some() {
                new_occurrences_to_create += 1;
            }
        }
    }

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
                    info!("Successfully generated QuickStatements file at: {:?}", output_path);
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
    println!("Total CSV records read (from initial validation): {}", processed_data.len() + errors_count); // Clarified this count
    println!("Successfully processed (passed enrichment and Wikidata checks): {}", processed_data.len());
    println!("New chemical entities to be created: {}", new_chemicals_to_create);
    println!("New occurrence statements to be created: {}", new_occurrences_to_create);
    
    println!("Errors encountered during processing: {}", errors_count);
    
    if !error_details.is_empty() { // <<< ADD THIS BLOCK
        println!("\n--- Detailed Errors ---");
        for detail in error_details {
            println!("- {}", detail);
        }
    }
    
    // TODO: Add more detailed stats (new chemicals, new occurrences, etc.)
    println!("Execution time: {:.2?}", duration);


    Ok(())
}

