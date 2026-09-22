//! Execute the `{python}` cells of a markdown document into a
//! content-addressed store of Jupyter outputs.

mod markdown;
mod outputs;
mod python;

use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use outputs::Store;

const USAGE: &str = "\
usage: slides-rs [INPUT] [--out DIR] [--python EXE] [--force]

    INPUT         markdown document to read (default: index.md)
    --out DIR     where to write the outputs (default: outputs)
    --python EXE  interpreter to run the cells with (default: python3)
    --force       re-run cells whose outputs are already stored
";

struct Args {
    input: PathBuf,
    out: PathBuf,
    interpreter: Option<String>,
    force: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        input: PathBuf::from("index.md"),
        out: PathBuf::from("outputs"),
        interpreter: None,
        force: false,
    };
    let mut input_seen = false;
    let mut argv = std::env::args().skip(1);
    while let Some(arg) = argv.next() {
        let mut value = |flag: &str| {
            argv.next()
                .ok_or_else(|| format!("{flag} needs a value\n\n{USAGE}"))
        };
        match arg.as_str() {
            "--out" => args.out = value("--out")?.into(),
            "--python" => args.interpreter = Some(value("--python")?),
            "--force" => args.force = true,
            "-h" | "--help" => return Err(USAGE.to_string()),
            flag if flag.starts_with('-') => {
                return Err(format!("unknown option {flag}\n\n{USAGE}"));
            }
            _ if input_seen => return Err(format!("too many inputs\n\n{USAGE}")),
            _ => {
                args.input = arg.into();
                input_seen = true;
            }
        }
    }
    Ok(args)
}

fn main() -> ExitCode {
    match run() {
        Ok(failed) if failed > 0 => {
            eprintln!("{failed} cell(s) raised");
            ExitCode::FAILURE
        }
        Ok(_) => ExitCode::SUCCESS,
        Err(message) if message == USAGE => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

/// Runs every cell and returns how many of them raised.
fn run() -> Result<usize, String> {
    let args = parse_args()?;
    let source = fs::read_to_string(&args.input)
        .map_err(|e| format!("could not read {}: {e}", args.input.display()))?;
    let cells = markdown::python_cells(&source);
    if cells.is_empty() {
        println!("no `{{python}}` cells in {}", args.input.display());
        return Ok(0);
    }

    let store = Store::new(&args.out)?;
    let interpreter = match args.interpreter {
        Some(interpreter) => interpreter,
        None => python::find_interpreter()?,
    };

    let mut failed = 0;
    for cell in &cells {
        let hash = outputs::hash(&cell.code);
        let at = format!("{}:{}", args.input.display(), cell.line);

        let cell_outputs = if args.force || !store.contains(&hash) {
            let cell_outputs = python::run_cell(&interpreter, &cell.code)?;
            let written = store.write(&hash, &cell_outputs)?;
            let written: Vec<_> = written.iter().map(|p| p.display().to_string()).collect();
            println!("{at}: ran {hash} -> {}", written.join(", "));
            cell_outputs
        } else {
            println!("{at}: cached {hash}");
            store.read(&hash)?
        };

        for error in cell_outputs
            .iter()
            .filter(|output| output["output_type"] == "error")
        {
            failed += 1;
            let name = error["ename"].as_str().unwrap_or("error");
            let value = error["evalue"].as_str().unwrap_or_default();
            eprintln!("{at}: {name}: {value}");
        }
    }
    Ok(failed)
}
