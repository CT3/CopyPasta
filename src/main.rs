use clap::{Arg, ArgGroup, Command};
use dirs::config_dir;
use indicatif::{ProgressBar, ProgressStyle};
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

    // Renames are instant, so only the ones that fail (e.g. across
    // filesystems, where `rename` returns `EXDEV`) fall back to a copy with
    // a progress bar, followed by removing the source.
    let mut pending = Vec::new();
    for (src, destination) in jobs(paths, &current_dir) {
        if fs::rename(src, &destination).is_ok() {
            println!("Moved {} to {}", src.display(), destination.display());
        } else {
            pending.push((src, destination));
        }
    }

    if pending.is_empty() {
        return Ok(());
    }

    let mut progress = Progress::new(pending.iter().map(|(src, _)| *src));
    for (src, destination) in &pending {
        match copy_recursively(src, destination, &mut progress).and_then(|()| remove_path(src)) {
            Ok(()) => progress.println(format!(
                "Moved {} to {}",
                src.display(),
                destination.display()
            )),
            Err(e) => progress.eprintln(format!("Error: Failed to move {}: {}", src.display(), e)),
        }
    }
    progress.finish();
    Ok(())
}

fn do_paste(paths: &[PathBuf]) -> io::Result<()> {
    let current_dir = std::env::current_dir()?;
    println!("Pasting files:");

    let jobs = jobs(paths, &current_dir);
    let mut progress = Progress::new(jobs.iter().map(|(src, _)| *src));
    for (src, destination) in &jobs {
        match copy_recursively(src, destination, &mut progress) {
            Ok(()) => progress.println(format!(
                "Pasted {} to {}",
                src.display(),
                destination.display()
            )),
            Err(e) => progress.eprintln(format!("Error: Failed to paste {}: {}", src.display(), e)),
        }
    }
    progress.finish();
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

/// Pair each source with its destination in `current_dir`, dropping the
/// ones `destination_for` skips.
fn jobs<'a>(paths: &'a [PathBuf], current_dir: &Path) -> Vec<(&'a Path, PathBuf)> {
    paths
        .iter()
        .filter_map(|src| destination_for(src, current_dir).map(|dst| (src.as_path(), dst)))
        .collect()
}

fn remove_path(src: &Path) -> io::Result<()> {
    if src.is_dir() {
        fs::remove_dir_all(src)
    } else {
        fs::remove_file(src)
    }
}

/// Files below this size are copied with `fs::copy` (which can use
/// copy-on-write clones); larger ones are streamed so the bar moves.
const STREAM_THRESHOLD: u64 = 4 * 1024 * 1024;

/// Progress over a whole paste/move: a byte-based bar, prefixed with the
/// number of files copied out of the total. Drawn on stderr and hidden
/// automatically when it isn't a terminal.
struct Progress {
    bar: ProgressBar,
    files_done: u64,
    files_total: u64,
}

impl Progress {
    /// Walk the sources up front to learn the total file count and size.
    fn new<'a>(sources: impl Iterator<Item = &'a Path>) -> Self {
        let (mut files_total, mut bytes_total) = (0, 0);
        for src in sources {
            tally(src, &mut files_total, &mut bytes_total);
        }

        let bar = ProgressBar::new(bytes_total);
        bar.set_style(
            ProgressStyle::with_template(
                "{prefix} files [{bar:30.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta}) {wide_msg}",
            )
            .expect("valid progress template")
            .progress_chars("=> "),
        );

        let progress = Progress {
            bar,
            files_done: 0,
            files_total,
        };
        progress.update_prefix();
        progress
    }

    fn update_prefix(&self) {
        self.bar
            .set_prefix(format!("{}/{}", self.files_done, self.files_total));
    }

    fn start_file(&self, src: &Path) {
        let name = src.file_name().unwrap_or(src.as_os_str());
        self.bar.set_message(name.to_string_lossy().into_owned());
    }

    fn finish_file(&mut self) {
        self.files_done += 1;
        self.update_prefix();
    }

    /// Print above the bar, or plainly when the bar is hidden.
    fn println(&self, line: String) {
        self.bar.suspend(|| println!("{}", line));
    }

    fn eprintln(&self, line: String) {
        self.bar.suspend(|| eprintln!("{}", line));
    }

    fn finish(&self) {
        self.bar.finish_and_clear();
        if self.files_total > 0 {
            println!("Copied {}/{} files.", self.files_done, self.files_total);
        }
    }
}

/// Count the regular files and bytes under `src`. Unreadable entries are
/// ignored here; the copy itself reports them.
fn tally(src: &Path, files: &mut u64, bytes: &mut u64) {
    if src.is_dir() {
        let Ok(entries) = fs::read_dir(src) else {
            return;
        };
        for entry in entries.flatten() {
            tally(&entry.path(), files, bytes);
        }
    } else if let Ok(metadata) = fs::metadata(src) {
        if metadata.is_file() {
            *files += 1;
            *bytes += metadata.len();
        }
    }
}

/// Recursively copy a file or directory tree from `src` to `dst`. Inside a
/// directory, a failed entry is reported and skipped rather than aborting the
/// rest of the tree; the result is still an error if anything failed, so a
/// move keeps its source.
fn copy_recursively(src: &Path, dst: &Path, progress: &mut Progress) -> io::Result<()> {
    if !src.is_dir() {
        return copy_file(src, dst, progress);
    }

    match copy_tree(src, dst, progress) {
        0 => Ok(()),
        failed => Err(io::Error::other(format!(
            "{} item(s) could not be copied",
            failed
        ))),
    }
}

/// Copy `src` to `dst`, reporting each failure and returning how many there were.
fn copy_tree(src: &Path, dst: &Path, progress: &mut Progress) -> u64 {
    let report = |progress: &Progress, path: &Path, e: io::Error| {
        progress.eprintln(format!("Error: Failed to copy {}: {}", path.display(), e));
        1
    };

    if !src.is_dir() {
        return match copy_file(src, dst, progress) {
            Ok(()) => 0,
            Err(e) => report(progress, src, e),
        };
    }

    let entries = match create_dir_all(dst).and_then(|()| fs::read_dir(src)) {
        Ok(entries) => entries,
        Err(e) => return report(progress, src, e),
    };

    let mut failed = 0;
    for entry in entries {
        failed += match entry {
            Ok(entry) => copy_tree(&entry.path(), &dst.join(entry.file_name()), progress),
            Err(e) => report(progress, src, e),
        };
    }
    failed
}

/// Copy one regular file. Sockets, FIFOs and devices (e.g. git's
/// `fsmonitor--daemon.ipc`) can't be copied as files, so they are skipped.
fn copy_file(src: &Path, dst: &Path, progress: &mut Progress) -> io::Result<()> {
    let metadata = fs::metadata(src)?;
    if !metadata.is_file() {
        progress.eprintln(format!("Skipping {}: not a regular file", src.display()));
        return Ok(());
    }

    if let Some(parent) = dst.parent() {
        create_dir_all(parent)?;
    }

    progress.start_file(src);
    let len = metadata.len();
    if len < STREAM_THRESHOLD {
        fs::copy(src, dst)?;
        progress.bar.inc(len);
    } else {
        let reader = File::open(src)?;
        let permissions = reader.metadata()?.permissions();
        let mut reader = BufReader::with_capacity(1024 * 1024, reader);
        let mut writer = progress.bar.wrap_write(File::create(dst)?);
        io::copy(&mut reader, &mut writer)?;
        fs::set_permissions(dst, permissions)?;
    }

    progress.finish_file();
    Ok(())
}
