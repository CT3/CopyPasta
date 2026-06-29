use clap::{Arg, ArgGroup, Command};
use dirs::config_dir;
use std::fs::{self, canonicalize, create_dir_all, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

fn main() -> io::Result<()> {
    let matches = Command::new(env!("CARGO_PKG_NAME"))
        .version(env!("CARGO_PKG_VERSION"))
        .author(env!("CARGO_PKG_AUTHORS"))
        .about("Copy and paste files")
        .arg(
            Arg::new("copy")
                .short('c')
                .long("copy")
                .num_args(1..) // Requires at least one argument
                .value_name("FILE(s)")
                .help("File(s) or folder(s) to copy"),
        )
        .arg(
            Arg::new("paste")
                .short('p')
                .long("paste")
                .num_args(0)
                .help("Paste the last copied file(s) or folder(s) to the current directory"),
        )
        .arg(
            Arg::new("move")
                .short('m')
                .long("move")
                .num_args(0)
                .help("Move the last tagged file(s) or folder(s) to the current directory"),
        )
        .arg(
            Arg::new("info")
                .short('i')
                .long("info")
                .num_args(0)
                .help("Show information about the last tagged file(s) or folder(s)"),
        )
        // Require exactly one action from the group
        .group(
            ArgGroup::new("actions")
                .args(["copy", "paste", "move", "info"])
                .required(true)
                .multiple(false),
        )
        .get_matches();

    let store = store_path()?;

    if let Some(files) = matches.get_many::<String>("copy") {
        do_copy(files.map(String::as_str), &store)
    } else if matches.get_flag("move") {
        do_move(&read_store(&store)?)
    } else if matches.get_flag("paste") {
        do_paste(&read_store(&store)?)
    } else if matches.get_flag("info") {
        do_info(&read_store(&store)?);
        Ok(())
    } else {
        // Unreachable: the ArgGroup guarantees exactly one action.
        unreachable!("no action selected despite required ArgGroup")
    }
}

/// Resolve `<config_dir>/copypasta/file_paths.txt`, creating the directory.
fn store_path() -> io::Result<PathBuf> {
    let config = config_dir().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "Could not find the user's config directory.",
        )
    })?;

    let dir = config.join("copypasta");
    create_dir_all(&dir)?;
    Ok(dir.join("file_paths.txt"))
}

/// Canonicalize the given paths and write the valid ones to the store.
/// The store is only truncated once at least one path resolves, so a fully
/// invalid invocation leaves the previously tagged paths intact.
fn do_copy<'a>(files: impl Iterator<Item = &'a str>, store: &Path) -> io::Result<()> {
    let mut copied_paths = Vec::new();

    for file in files {
        match canonicalize(file) {
            Ok(full_path) => copied_paths.push(full_path),
            Err(e) => eprintln!("Error resolving full path for {}: {}", file, e),
        }
    }

    if copied_paths.is_empty() {
        eprintln!("Error: No valid paths were copied.");
        std::process::exit(1);
    }

    let mut writer = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(store)?;

    println!("Copied paths:");
    for path in &copied_paths {
        writeln!(writer, "{}", path.display())?;
        println!("{}", path.display());
    }

    Ok(())
}

/// Read the tagged paths from the store, erroring (rather than silently
/// skipping) on an unreadable line, and exiting if the store is empty.
fn read_store(store: &Path) -> io::Result<Vec<PathBuf>> {
    let reader = BufReader::new(File::open(store)?);
    let mut paths = Vec::new();
    for line in reader.lines() {
        paths.push(PathBuf::from(line?));
    }

    if paths.is_empty() {
        eprintln!("Error: No file path(s) found in {}", store.display());
        eprintln!("Please use the '-c' option first to copy file(s).");
        std::process::exit(1);
    }

    Ok(paths)
}

fn do_move(paths: &[PathBuf]) -> io::Result<()> {
    let current_dir = std::env::current_dir()?;
    println!("Moving files:");
    for src in paths {
        let Some(destination) = destination_for(src, &current_dir) else {
            continue;
        };

        match move_path(src, &destination) {
            Ok(()) => println!("Moved {} to {}", src.display(), destination.display()),
            Err(e) => eprintln!("Error: Failed to move {}: {}", src.display(), e),
        }
    }
    Ok(())
}

fn do_paste(paths: &[PathBuf]) -> io::Result<()> {
    let current_dir = std::env::current_dir()?;
    println!("Pasting files:");
    for src in paths {
        let Some(destination) = destination_for(src, &current_dir) else {
            continue;
        };

        match copy_recursively(src, &destination) {
            Ok(()) => println!("Pasted {} to {}", src.display(), destination.display()),
            Err(e) => eprintln!("Error: Failed to paste {}: {}", src.display(), e),
        }
    }
    Ok(())
}

fn do_info(paths: &[PathBuf]) {
    println!("Information about copied paths:");
    for path in paths {
        println!("{}", path.display());
    }
}

/// Compute the destination path in `current_dir` for `src`, or `None` if it
/// should be skipped (no file name, or source and destination are the same).
fn destination_for(src: &Path, current_dir: &Path) -> Option<PathBuf> {
    let Some(file_name) = src.file_name() else {
        eprintln!("Error: Could not get filename for {}", src.display());
        return None;
    };

    let destination = current_dir.join(file_name);
    if destination == src {
        eprintln!(
            "Skipping {}: source and destination are the same.",
            src.display()
        );
        return None;
    }

    Some(destination)
}

/// Move `src` to `dst`, falling back to copy-then-remove when a plain rename
/// fails (e.g. across filesystems, where `rename` returns `EXDEV`).
fn move_path(src: &Path, dst: &Path) -> io::Result<()> {
    if fs::rename(src, dst).is_ok() {
        return Ok(());
    }

    copy_recursively(src, dst)?;
    if src.is_dir() {
        fs::remove_dir_all(src)
    } else {
        fs::remove_file(src)
    }
}

/// Recursively copy a file or directory tree from `src` to `dst`.
fn copy_recursively(src: &Path, dst: &Path) -> io::Result<()> {
    if src.is_dir() {
        create_dir_all(dst)?;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            copy_recursively(&entry.path(), &dst.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        if let Some(parent) = dst.parent() {
            create_dir_all(parent)?;
        }
        fs::copy(src, dst).map(|_| ())
    }
}
