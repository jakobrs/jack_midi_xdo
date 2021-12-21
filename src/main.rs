use std::fs::File;
use std::io::Read;
use std::ops::Deref;
use std::{collections::HashMap, path::PathBuf};

use anyhow::Context;
use jack::{Client, ClientOptions, ClosureProcessHandler, ProcessScope};
use libxdo::XDo;
use serde::Deserialize;
use serde_with::{serde_as, DisplayFromStr};
use structopt::StructOpt;
use wmidi::MidiMessage;

#[derive(StructOpt)]
struct Opts {
    #[structopt(short, long, default_value = "config.toml")]
    config: PathBuf,

    #[structopt(long)]
    display: Option<String>,
}

#[serde_as]
#[derive(Deserialize)]
struct Config {
    meta: Option<Meta>,

    #[serde_as(as = "HashMap<DisplayFromStr, _>")]
    keybinds: HashMap<u8, String>,
}

#[derive(Deserialize)]
struct Meta {
    game: Option<String>,
}

#[repr(transparent)]
struct SendXDo(XDo);

unsafe impl Send for SendXDo {}

impl Deref for SendXDo {
    type Target = XDo;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

fn main() -> anyhow::Result<()> {
    env_logger::builder().format_timestamp(None).init();

    let opts = Opts::from_args();

    let mut config_contents = Vec::new();
    File::open(opts.config)
        .context("config.toml does not exist")?
        .read_to_end(&mut config_contents)?;
    let config: Config = toml::from_slice(config_contents.as_slice())?;

    if let Some(meta) = config.meta {
        if let Some(game) = meta.game {
            println!("Using keybinds for game {:?}", game);
        }
    }

    let xdo = SendXDo(XDo::new(opts.display.as_deref())?);

    let (client, _client_status) = jack::Client::new("jack_midi_xdo", ClientOptions::NO_START_SERVER)?;
    let midi_in = client.register_port("in", jack::MidiIn::default())?;

    let process = move |_client: &Client, ps: &ProcessScope| -> jack::Control {
        for msg in midi_in.iter(ps) {
            match MidiMessage::try_from(msg.bytes) {
                Ok(MidiMessage::NoteOn(_ch, note, _v)) => {
                    if let Some(action) = config.keybinds.get(&(note as u8)) {
                        if let Err(err) = xdo.send_keysequence_down(action, 0) {
                            log::error!("Unable to send keyseq down: {:?}", err);
                        }
                    } else {
                        log::info!("Unmapped note {:?}", note);
                    }
                }
                Ok(MidiMessage::NoteOff(_ch, note, _v)) => {
                    if let Some(action) = config.keybinds.get(&(note as u8)) {
                        if let Err(err) = xdo.send_keysequence_up(action, 0) {
                            log::error!("Unable to send keyseq up: {:?}", err);
                        }
                    }
                }
                Ok(_) => {}
                Err(err) => {
                    log::error!("Unable to parse MIDI message: {:?}", err);
                }
            }
        }

        jack::Control::Continue
    };

    let _async_client =
        client.activate_async((), ClosureProcessHandler::new(process))?;

    // Just waits for enter to be pressed
    std::io::stdin().read_line(&mut String::new())?;

    Ok(())
}
