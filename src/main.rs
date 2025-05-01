use clap::{Arg, Command}; // Removed value_parser
use dirs::config_dir;
use std::fs::{canonicalize, create_dir_all, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::Command as ProcessCommand;

fn main() -> io::Result<()> {
    // Define the command-line arguments and options
    let matches = Command::new("clap_demo")
        .version("1.0")
        .author("Mantas Jurkuvenas")
        .about("Copy and paste files")
        .arg(
            Arg::new("copy")
                .short('c')
                .long("copy")
                // Accept one or more values
                .num_args(1..) // Requires at least one argument
                .value_name("FILE(s)")
                .help("File(s) or folder(s) to copy"),
        )
        .arg(
            Arg::new("paste")
                .short('p')
                .long("paste")
                .num_args(0) // No value expected
                .help("Paste the last copied file(s) or folder(s) to the current directory"),
        )
        .arg(
            Arg::new("move")
                .short('m')
                .long("move")
                .num_args(0) // No value expected
                .help("Move the last tagged file(s) or folder(s) to the current directory"),
        )
        .arg(
            Arg::new("info")
                .short('i')
                .long("info")
                .num_args(0) // No value expected
                .help("Show information about the last tagged file(s) or folder(s)"),
        )
        // Require exactly one action from the group
        .group(
            clap::ArgGroup::new("actions")
                .args(&["copy", "paste", "move", "info"])
                .required(true)
                .multiple(false), // Only one action at a time
        )
        .get_matches();

    // Get the path to the user's config directory
    if let Some(config_path) = config_dir() {
        // Construct the full path to ~/.config/copypasta
        let mut copypasta_dir = config_path.clone();
        copypasta_dir.push("copypasta");

        create_dir_all(&copypasta_dir)?;

        let mut file_path_store = copypasta_dir.clone();
        file_path_store.push("file_paths.txt");

        // Determine the action based on which argument is present
        if let Some(files_to_copy_iter) = matches.get_many::<String>("copy") {
            // Handle the copy action
            let files_to_copy: Vec<&String> = files_to_copy_iter.collect();

            if files_to_copy.is_empty() {
                 eprintln!("Error: No file(s) provided for copy.");
                 std::process::exit(1);
            }

            let mut file_writer = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true) // Truncate the file when writing the new path(s)
                .open(&file_path_store)?;

            let mut copied_paths = Vec::new();

            for file_to_copy in files_to_copy {
                // Resolve the full path of the file
                match canonicalize(file_to_copy) {
                    Ok(full_path) => {
                        // Save the full path of the file being copied to the config file
                        writeln!(file_writer, "{}", full_path.display())?;
                        copied_paths.push(full_path);
                    }
                    Err(e) => {
                        eprintln!("Error resolving full path for {}: {}", file_to_copy, e);
                        // Decide if you want to continue or exit on error
                        // For now, we'll print the error and continue with other files
                    }
                }
            }

            if !copied_paths.is_empty() {
                 println!("Copied paths:");
                 for path in copied_paths {
                     println!("{}", path.display());
                 }
            } else {
                 eprintln!("Error: No valid paths were copied.");
                 std::process::exit(1);
            }


        } else {
            // Actions that require reading from the file_path_store
            let file = File::open(&file_path_store)?;
            let reader = BufReader::new(file);
            let paths_to_act_on: Vec<PathBuf> = reader
                .lines()
                .filter_map(Result::ok)
                .map(PathBuf::from) // Convert String to PathBuf
                .collect();

            if paths_to_act_on.is_empty() {
                eprintln!("Error: No file path(s) found in {}", file_path_store.display());
                eprintln!("Please use the '-c' option first to copy file(s).");
                std::process::exit(1);
            }

            if matches.get_flag("move") {
                // Handle the move action for all paths
                println!("Moving files:");
                for last_path in paths_to_act_on {
                    if let Some(file_name) = last_path.file_name() {
                         let destination = std::env::current_dir()?.join(file_name);

                         let status = ProcessCommand::new("mv")
                             .arg(&last_path)
                             .arg(&destination)
                             .status()?;

                         if status.success() {
                             println!("Moved {} to {}", last_path.display(), destination.display());
                         } else {
                             eprintln!(
                                 "Error: Failed to move {}. Status: {:?}",
                                 last_path.display(),
                                 status
                             );
                         }
                    } else {
                         eprintln!("Error: Could not get filename for {}", last_path.display());
                    }
                }

            } else if matches.get_flag("paste") {
                // Handle the paste action for all paths
                println!("Pasting files:");
                for last_path in paths_to_act_on {
                     if let Some(file_name) = last_path.file_name() {
                         let destination = std::env::current_dir()?.join(file_name);

                         // Use -r for recursive copy in case of directories
                         let status = ProcessCommand::new("cp")
                             .arg("-r")
                             .arg(&last_path)
                             .arg(&destination)
                             .status()?;

                         if status.success() {
                             println!("Pasted {} to {}", last_path.display(), destination.display());
                         } else {
                             eprintln!(
                                 "Error: Failed to paste {}. Status: {:?}",
                                 last_path.display(),
                                 status
                             );
                         }
                     } else {
                         eprintln!("Error: Could not get filename for {}", last_path.display());
                     }
                }

            } else if matches.get_flag("info") {
                // Handle the info action for all paths
                println!("Information about copied paths:");
                for path in paths_to_act_on {
                    println!("{}", path.display());
                }
            }
        }
    } else {
        eprintln!("Error: Could not find the user's config directory.");
        std::process::exit(1);
    }

    Ok(())
}
