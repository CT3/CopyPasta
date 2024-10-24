use clap::{Arg, Command};
use dirs::config_dir;
use std::fs::{canonicalize, create_dir_all, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::process::Command as ProcessCommand;

fn main() -> io::Result<()> {
    // Define the command-line arguments and options
    let matches = Command::new("clap_demo")
        .version("1.0")
        .author("Mantas Jurkuvenas")
        .about("Copy and paste files")
        .arg(
            Arg::new("copy")
                .short('c') // Changed from "cp" to "-c"
                .long("copy")
                .num_args(1) // Replaces `takes_value(true)`
                .value_name("FILE")
                .help("File or folder to copy (required when using -c)"),
        )
        .arg(
            Arg::new("paste")
                .short('p') // Changed from "paste" to "-p"
                .long("paste")
                .num_args(0) // Replaces `takes_value(false)`
                .help("Paste the last copied file or folder"),
        )
        .arg(
            Arg::new("move")
                .short('m') // Changed from "paste" to "-p"
                .long("move")
                .num_args(0) // Replaces `takes_value(false)`
                .help("Move the last tagged file or folder"),
        )
        // Make either `-c` or `-p` required
        .group(
            clap::ArgGroup::new("actions")
                .args(&["copy", "paste", "move"])
                .required(true),
        ) // At least one of them must be provided
        .get_matches();

    // Determine the action
    let file = matches.get_one::<String>("copy");
    let is_move = matches.get_flag("move"); // Check for move first
    let is_paste = matches.get_flag("paste"); // Check for paste second

    // Get the path to the user's config directory
    if let Some(config_path) = config_dir() {
        // Construct the full path to ~/.config/copypasta
        let mut copypasta_dir = config_path.clone();
        copypasta_dir.push("copypasta");

        create_dir_all(&copypasta_dir)?;

        let mut file_path = copypasta_dir.clone();
        file_path.push("file_paths.txt");

        if let Some(file_to_copy) = file {
            // Resolve the full path of the file
            match canonicalize(file_to_copy) {
                Ok(full_path) => {
                    let mut file_writer = OpenOptions::new()
                        .write(true)
                        .create(true)
                        .truncate(true) // Truncate the file when writing the new path
                        .open(&file_path)?;

                    // Save the full path of the file being copied to the config file
                    writeln!(file_writer, "{}", full_path.display())?;
                    println!("Copied: {}", full_path.display());
                }
                Err(e) => {
                    eprintln!("Error resolving full path: {}", e);
                    std::process::exit(1);
                }
            }
        } else if is_move {
            // Read the last saved file path from file_paths.txt
            let file = File::open(&file_path)?;
            let reader = BufReader::new(file);
            let paths: Vec<String> = reader.lines().filter_map(Result::ok).collect();

            if let Some(last_path) = paths.last() {
                // Attempt to move the file to the current directory
                let destination =
                    format!("./{}", last_path.split('/').last().unwrap_or(&last_path));
                let status = ProcessCommand::new("mv")
                    .arg(last_path)
                    .arg(&destination)
                    .status()?;

                if status.success() {
                    println!("Moved {} to {}", last_path, destination);
                } else {
                    eprintln!("Error: Failed to move the file.");
                }
            } else {
                eprintln!("Error: No valid file path found in {}", file_path.display());
                eprintln!("Please ensure that the file is not empty and contains valid paths.");
            }
        } else if is_paste {
            // Read the last saved file path from file_paths.txt
            let file = File::open(&file_path)?;
            let reader = BufReader::new(file);
            let paths: Vec<String> = reader.lines().filter_map(Result::ok).collect();

            if let Some(last_path) = paths.last() {
                // Attempt to copy the file to the current directory
                let destination =
                    format!("./{}", last_path.split('/').last().unwrap_or(&last_path));
                let status = ProcessCommand::new("cp")
                    .arg("-r")
                    .arg(last_path)
                    .arg(&destination)
                    .status()?;

                if status.success() {
                    println!("Pasted {} to {}", last_path, destination);
                } else {
                    eprintln!("Error: Failed to paste the file.");
                }
            } else {
                eprintln!("Error: No valid file path found in {}", file_path.display());
                eprintln!("Please ensure that the file is not empty and contains valid paths.");
            }
        } else {
            eprintln!("Error: Invalid action. Use '-c' to copy, '-p' to paste, or '-m' to move.");
            std::process::exit(1);
        }
    } else {
        eprintln!("Error: Could not find the user's config directory.");
        std::process::exit(1);
    }

    Ok(())
}

