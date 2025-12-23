use clap::Parser;
use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use toxxi::model::Message;

#[derive(Parser, Debug)]
#[command(author, version, about = "Cleans and deduplicates toxxi chat logs", long_about = None)]
struct Args {
    /// Path to the input log file (.jsonl)
    input: PathBuf,

    /// Path to the output file. Defaults to <input>.clean
    #[arg(short, long, conflicts_with = "in_place")]
    output: Option<PathBuf>,

    /// If set, overwrites the input file with the cleaned version
    #[arg(short, long)]
    in_place: bool,

    /// If set, don't write any files, just show statistics
    #[arg(short = 'n', long)]
    dryrun: bool,
}

fn main() -> io::Result<()> {
    let args = Args::parse();

    let input_path = &args.input;
    let output_path = if args.in_place {
        input_path.with_extension("jsonl.tmp")
    } else {
        match &args.output {
            Some(p) => p.clone(),
            None => input_path.with_extension("jsonl.clean"),
        }
    };

    let content = fs::read_to_string(input_path)?;
    let mut messages = HashMap::new();

    let mut stats_total_lines = 0;
    let mut stats_corrupted_lines = 0;
    let mut stats_total_objects = 0;
    let mut stats_status_upgrades = 0;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        stats_total_lines += 1;

        // Handle corrupted lines where multiple JSON objects are smashed together
        // e.g. {"status":"Received"...}{"status":"Received"...}
        let segments: Vec<&str> = if line.contains("}{") {
            stats_corrupted_lines += 1;
            line.split("}{").collect()
        } else {
            vec![line]
        };

        for (i, segment) in segments.iter().enumerate() {
            stats_total_objects += 1;
            let mut s = segment.to_string();
            if segments.len() > 1 {
                if i == 0 {
                    s.push('}');
                } else if i == segments.len() - 1 {
                    s.insert(0, '{');
                } else {
                    s.insert(0, '{');
                    s.push('}');
                }
            }

            if let Ok(msg) = serde_json::from_str::<Message>(&s) {
                let key = (msg.timestamp, msg.internal_id);
                messages
                    .entry(key)
                    .and_modify(|existing: &mut Message| {
                        if status_priority(&msg.status) > status_priority(&existing.status) {
                            stats_status_upgrades += 1;
                            *existing = msg.clone();
                        }
                    })
                    .or_insert(msg);
            }
        }
    }

    let mut result: Vec<Message> = messages.into_values().collect();
    result.sort_by_key(|m| (m.timestamp, m.internal_id));

    println!("Log Cleaning Statistics for {:?}:", input_path);
    println!("  Total lines read:      {}", stats_total_lines);
    println!("  Corrupted lines found: {}", stats_corrupted_lines);
    println!("  Total JSON objects:    {}", stats_total_objects);
    println!("  Unique messages:       {}", result.len());
    println!(
        "  Deduplicated objects:  {}",
        stats_total_objects - result.len()
    );
    println!("  Status upgrades:       {}", stats_status_upgrades);

    if args.dryrun {
        println!("\nDry run completed. No files were written.");
        return Ok(());
    }

    let mut out_file = fs::File::create(&output_path)?;
    for msg in &result {
        let json = serde_json::to_string(&msg).unwrap();
        out_file.write_all(json.as_bytes())?;
        out_file.write_all(b"\n")?;
    }

    if args.in_place {
        fs::rename(&output_path, input_path)?;
        println!(
            "\nCleaned {} messages. File {:?} updated in-place.",
            result.len(),
            input_path
        );
    } else {
        println!(
            "\nCleaned {} messages. Saved to {:?}",
            result.len(),
            output_path
        );
        if args.output.is_none() {
            println!(
                "To replace original file, run: mv {:?} {:?}",
                output_path, input_path
            );
        }
    }

    Ok(())
}

fn status_priority(status: &toxxi::model::MessageStatus) -> i32 {
    use toxxi::model::MessageStatus;
    match status {
        MessageStatus::Received => 4,
        MessageStatus::Sent(_) => 3,
        MessageStatus::Sending => 2,
        MessageStatus::Pending => 1,
        MessageStatus::Incoming => 0, // Should not happen for our own messages usually
        MessageStatus::Failed => -1,
    }
}
