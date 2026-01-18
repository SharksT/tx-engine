use std::env;
use std::error::Error;
use std::fs::File;
use std::io;

use csv::{ReaderBuilder, Trim, Writer};

use tx_engine::{Engine, Transaction};

fn run(input_path: &str) -> Result<(), Box<dyn Error>> {
    let file = File::open(input_path)?;
    let mut reader = ReaderBuilder::new()
        .trim(Trim::All)
        .flexible(true)
        .from_reader(file);

    let mut engine = Engine::new();

    for result in reader.deserialize() {
        let tx: Transaction = result?;
        engine.process(tx);
    }

    let mut writer = Writer::from_writer(io::stdout());
    for account in engine.output() {
        writer.serialize(account)?;
    }
    writer.flush()?;

    Ok(())
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() != 2 {
        eprintln!("Usage: {} <transactions.csv>", args[0]);
        std::process::exit(1);
    }

    if let Err(e) = run(&args[1]) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
