use clap::{Arg, Command};
use std::fs::{create_dir_all, canonicalize, OpenOptions, File};
use std::io::{self, Write, BufReader, BufRead};
use std::process::Command as ProcessCommand;
use dirs::config_dir;

fn main() -> io::Result<()> {
    // Define the command-line arguments and options
    let matches = Command::new("clap_demo")
        .version("1.0")
        .author("Your Name <your.email@example.com>")
        .about("A simple CLI demo using Clap")
        .arg(
            Arg::new("action") // First positional argument
                .value_name("ACTION")
                .help("Action to perform: cp or paste")
                .required(true),
        )
        .arg(
            Arg::new("file") // Second positional argument
                .value_name("FILE")
                .help("File or folder to copy (required if action is cp)")
                .required(false), // Initially, this is not required
        )
        .get_matches();

    let action = matches
        .get_one::<String>("action")
        .map(|v| v.to_lowercase())
        .unwrap_or_else(|| "paste".to_string());

    let file = matches.get_one::<String>("file");

    // Get the path to the user's config directory
    if let Some(config_path) = config_dir() {
        // Construct the full path to ~/.config/copypasta
        let mut copypasta_dir = config_path.clone();
        copypasta_dir.push("copypasta");

        // Create ~/.config/copypasta if it doesn't exist
        create_dir_all(&copypasta_dir)?;

        // The path to the file where we will save the file path
        let mut file_path = copypasta_dir.clone();
        file_path.push("file_paths.txt");

        // If the action is "cp", write the file path to file_paths.txt
        if action == "cp" {
            if let Some(file_to_copy) = file {
                // Resolve the full path of the file
                match canonicalize(file_to_copy) {
                    Ok(full_path) => {
                        // Open the file for appending (create if it doesn't exist)
                        let mut file_writer = OpenOptions::new()
                            .write(true)
                            .create(true)
                            .append(true) // Append to the file instead of truncating
                            .open(&file_path)?;

                        // Save the full path of the file being copied to the config file
                        writeln!(file_writer, "{}", full_path.display())?;
                        // println!("Copied");
                    }
                    Err(e) => {
                        eprintln!("Error resolving full path: {}", e);
                        std::process::exit(1);
                    }
                }
            } else {
                eprintln!("Error: A file must be specified when the action is 'cp'.");
                std::process::exit(1);
            }
        } else if action == "paste" {
            // Read the last saved file path from file_paths.txt
            let file = File::open(&file_path)?;
            let reader = BufReader::new(file);
            let paths: Vec<String> = reader.lines()
                .filter_map(Result::ok)
                .collect();

            if let Some(last_path) = paths.last() {
                // Attempt to copy the file to the current directory
                let destination = format!("./{}", last_path.split('/').last().unwrap_or(&last_path));
                let status = ProcessCommand::new("cp")
                    .arg("-r")
                    .arg(last_path)
                    .arg(&destination)
                    .status()?;

                if status.success() {
                    println!("Copied {} to {}", last_path, destination);
                } else {
                    eprintln!("Error: Failed to copy the file.");
                }
            } else {
                eprintln!("Error: No valid file path found in {}", file_path.display());
                eprintln!("Please ensure that the file is not empty and contains valid paths.");
            }
        } else {
            eprintln!("Error: Invalid action. Use 'cp' or 'paste'.");
            std::process::exit(1);
        }
    } else {
        eprintln!("Error: Could not find the user's config directory.");
        std::process::exit(1);
    }

    Ok(())
}

