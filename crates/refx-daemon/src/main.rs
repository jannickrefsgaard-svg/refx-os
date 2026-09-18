//! `refxd` — REFX runtime host process.
//!
//! Usage:
//!   refxd [--config <path>] [--smoke]
//!
//! `--smoke` starts the runtime, runs one task through its lifecycle,
//! shuts down and exits 0 on success. Used by CI as an end-to-end check.
//! Without it, refxd runs until Enter is pressed (a signal-based shutdown
//! arrives with the Windows service host in Phase 2).

use std::path::PathBuf;
use std::process::ExitCode;

use refx_core::config::ConfigSource;
use refx_core::{logging, Priority, RefxConfig, Runtime, TaskState};

struct Args {
    config: PathBuf,
    smoke: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args { config: PathBuf::from("refx.toml"), smoke: false };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--config" => {
                args.config = it.next().ok_or("--config requires a path")?.into();
            }
            "--smoke" => args.smoke = true,
            "-h" | "--help" => {
                println!("usage: refxd [--config <path>] [--smoke]");
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument `{other}`")),
        }
    }
    Ok(args)
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            // Logger may not be up yet, so always print to stderr too.
            eprintln!("refxd: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args()?;
    let (config, source) = RefxConfig::load(&args.config)?;
    logging::init(&config.logging)?;

    match &source {
        ConfigSource::File(p) => tracing::info!(path = %p.display(), "configuration loaded"),
        ConfigSource::Defaults => tracing::info!(
            path = %args.config.display(),
            "no config file found; using defaults"
        ),
    }

    let platform = refx_platform::detect();
    let info = platform.system_info()?;
    tracing::info!(
        adapter = platform.adapter_name(),
        os = %info.os,
        arch = info.arch,
        cpus = info.logical_cpus,
        "platform detected"
    );

    let mut runtime = Runtime::new(config);
    runtime.start()?;
    tracing::info!(
        instance = %runtime.context().config.general.instance_name,
        "REFX runtime ready"
    );

    if args.smoke {
        smoke_test(&runtime)?;
    } else {
        println!("REFX runtime running. Press Enter to stop.");
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
    }

    runtime.shutdown()?;
    Ok(())
}

fn smoke_test(runtime: &Runtime) -> Result<(), Box<dyn std::error::Error>> {
    let tasks = &runtime.context().tasks;
    let id = tasks.create("smoke test", Priority::Low)?;
    for s in [TaskState::Queued, TaskState::Executing, TaskState::Completed] {
        tasks.transition(id, s, Some("smoke"))?;
    }
    let state = tasks.get(id).map(|t| t.state);
    if state != Some(TaskState::Completed) {
        return Err(format!("smoke task ended in {state:?}").into());
    }
    tracing::info!(task = %id, "smoke test passed");
    Ok(())
}
