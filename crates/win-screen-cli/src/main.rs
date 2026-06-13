use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use std::io::{self, Read};
use std::path::PathBuf;
use win_screen_core::{
    AudioOptions, CapturedImage, Capturer, InteractiveCaptureOptions, Pin, Recorder,
    RecordingTarget, Rect, Screenshot,
};

#[derive(Debug, Parser)]
#[command(name = "win-screen")]
#[command(about = "Windows screenshot, recording, and desktop pinning CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Shot(ShotArgs),
    Record(RecordArgs),
    Pin(PinArgs),
    ListMonitors,
    ListWindows,
}

#[derive(Debug, Args)]
struct ShotArgs {
    #[arg(long, conflicts_with = "fullscreen")]
    interactive: bool,

    #[arg(long)]
    fullscreen: bool,

    #[arg(long, conflicts_with_all = ["interactive", "fullscreen", "region", "window"])]
    monitor: Option<u32>,

    #[arg(long, conflicts_with_all = ["interactive", "fullscreen", "region", "monitor"])]
    window: Option<String>,

    #[arg(long, value_names = ["X", "Y", "WIDTH", "HEIGHT"], num_args = 4)]
    region: Option<Vec<i32>>,

    #[arg(long)]
    save: Option<PathBuf>,

    #[arg(long)]
    clipboard: bool,

    #[arg(long, help = "Open the annotation editor after an interactive capture")]
    annotate: bool,

    #[arg(long, help = "Skip the annotation editor for interactive captures")]
    no_annotate: bool,

    #[arg(long, help = "Create a desktop pin from the captured image")]
    pin: bool,

    #[arg(long, help = "Return immediately after creating a pin window")]
    no_wait: bool,
}

#[derive(Debug, Args)]
struct RecordArgs {
    #[arg(long)]
    output: PathBuf,

    #[arg(long, value_enum, value_delimiter = ',', default_value = "system")]
    audio: Vec<AudioSource>,
}

#[derive(Debug, Clone, ValueEnum)]
enum AudioSource {
    System,
    Mic,
}

#[derive(Debug, Args)]
struct PinArgs {
    #[arg(long)]
    file: Option<PathBuf>,

    #[arg(long)]
    clipboard: bool,

    #[arg(long)]
    list: bool,

    #[arg(long)]
    copy: Option<u64>,

    #[arg(long)]
    save: Option<u64>,

    #[arg(long)]
    output: Option<PathBuf>,

    #[arg(long)]
    opacity: Option<u64>,

    #[arg(long)]
    value: Option<f32>,

    #[arg(long, help = "Return immediately after creating the pin window")]
    no_wait: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    win_screen_core::platform::set_process_dpi_aware().ok();

    match cli.command {
        Command::Shot(args) => shot(args),
        Command::Record(args) => record(args),
        Command::Pin(args) => pin(args),
        Command::ListMonitors => list_monitors(),
        Command::ListWindows => list_windows(),
    }
}

fn shot(args: ShotArgs) -> Result<()> {
    let mut pin_requested = args.pin;
    let image = if args.interactive {
        let annotate = args.annotate && !args.no_annotate;
        let result = Capturer::interactive_with_result(InteractiveCaptureOptions {
            annotate,
            copy_to_clipboard: args.clipboard,
            save_path: args.save.clone(),
        })?
        .context("interactive capture was canceled")?;
        pin_requested |= result.pin_requested;
        result.image
    } else if let Some(region) = args.region {
        let rect = Rect::new(
            region[0],
            region[1],
            u32::try_from(region[2]).context("region width must be positive")?,
            u32::try_from(region[3]).context("region height must be positive")?,
        )?;
        Screenshot::capture_region(rect)?
    } else if let Some(monitor) = args.monitor {
        Screenshot::capture_monitor(monitor)?
    } else if let Some(window) = args.window.as_ref() {
        let hwnd = parse_hwnd(window)?;
        Screenshot::capture_window(hwnd)?
    } else {
        Screenshot::capture_fullscreen()?
    };

    if let Some(path) = args.save.as_ref() {
        image
            .save_png(path)
            .with_context(|| format!("failed to save {}", path.display()))?;
        println!("saved {}", path.display());
    }

    if args.clipboard {
        image.copy_to_clipboard()?;
        println!("copied {}x{} image to clipboard", image.width, image.height);
    }

    if pin_requested {
        let handle = Pin::from_image(image)?;
        println!("pin created: {}", handle.id());
        wait_for_pin_if_needed(&handle, args.no_wait)?;
        return Ok(());
    }

    if !args.clipboard && args.save.is_none() {
        println!("captured {}x{} image", image.width, image.height);
    }

    Ok(())
}

fn record(args: RecordArgs) -> Result<()> {
    let audio = AudioOptions {
        system: args
            .audio
            .iter()
            .any(|source| matches!(source, AudioSource::System)),
        microphone: args
            .audio
            .iter()
            .any(|source| matches!(source, AudioSource::Mic)),
    };

    let handle = Recorder::builder()
        .target(RecordingTarget::Fullscreen)
        .audio(audio)
        .output(args.output)
        .start()?;
    println!("recording started (id {}), press Enter to stop...", handle.id());
    let mut buf = [0u8; 1];
    let _ = io::stdin().read(&mut buf);
    let output = handle.stop()?;
    println!("saved {}", output.display());
    Ok(())
}

fn pin(args: PinArgs) -> Result<()> {
    if args.list {
        for pin in Pin::list()? {
            println!(
                "pin {}: image={}x{} window={}x{} at {},{} opacity={:.0}%",
                pin.id,
                pin.size.width,
                pin.size.height,
                pin.display_size.width,
                pin.display_size.height,
                pin.position.x,
                pin.position.y,
                pin.opacity * 100.0
            );
        }
        return Ok(());
    }

    if let Some(id) = args.copy {
        Pin::copy(id)?;
        println!("copied pin {id} to clipboard");
        return Ok(());
    }

    if let Some(id) = args.save {
        let output = args.output.context("pin --save requires --output <PATH>")?;
        Pin::save_png(id, &output)?;
        println!("saved pin {id} to {}", output.display());
        return Ok(());
    }

    if let Some(id) = args.opacity {
        let value = args
            .value
            .context("pin --opacity requires --value <0.1..1.0>")?;
        win_screen_core::pin::set_pin_opacity(id, value)?;
        println!("set pin {id} opacity to {value}");
        return Ok(());
    }

    if let Some(path) = args.file {
        let image = CapturedImage::load(&path)
            .with_context(|| format!("failed to load {}", path.display()))?;
        let handle = Pin::from_image(image)?;
        println!("pin created: {}", handle.id());
        wait_for_pin_if_needed(&handle, args.no_wait)?;
        return Ok(());
    }

    if args.clipboard {
        let handle = Pin::from_clipboard()?;
        println!("pin created: {}", handle.id());
        wait_for_pin_if_needed(&handle, args.no_wait)?;
        return Ok(());
    }

    anyhow::bail!("pass --clipboard or --file")
}

fn wait_for_pin_if_needed(handle: &win_screen_core::PinHandle, no_wait: bool) -> Result<()> {
    if no_wait {
        return Ok(());
    }

    println!("press Enter to close pin");
    let mut one = [0_u8; 1];
    let _ = io::stdin().read(&mut one)?;
    handle.close()?;
    Ok(())
}

fn parse_hwnd(value: &str) -> Result<isize> {
    let trimmed = value.trim();
    if let Some(hex) = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        isize::from_str_radix(hex, 16).context("invalid hex HWND")
    } else {
        trimmed.parse::<isize>().context("invalid HWND")
    }
}

fn list_monitors() -> Result<()> {
    for monitor in Screenshot::monitors()? {
        let primary = if monitor.primary { " primary" } else { "" };
        println!(
            "{}:{} x={} y={} w={} h={}",
            monitor.id,
            primary,
            monitor.rect.x,
            monitor.rect.y,
            monitor.rect.width,
            monitor.rect.height
        );
    }
    Ok(())
}

fn list_windows() -> Result<()> {
    for window in Screenshot::windows()? {
        println!(
            "0x{:X} x={} y={} w={} h={} {}",
            window.hwnd,
            window.rect.x,
            window.rect.y,
            window.rect.width,
            window.rect.height,
            window.title
        );
    }
    Ok(())
}
