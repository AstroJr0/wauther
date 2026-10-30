//! The command line: what `wauther <command>` means.
//!
//! ```text
//! wauther add <NAME> <KEY>
//! wauther add -n <NAME> -k <KEY>
//! wauther add -u <URI> [-n <NAME>]
//! wauther temp <KEY>
//! wauther temp -u <URI>
//! wauther run [-t]
//! wauther path [<PATH>]
//! ```

use std::error::Error;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::core::{Key, Settings, Uri, Vault};
use crate::tui::{Start, Temp};

/// A TOTP authenticator, in the terminal and on the desktop.
#[derive(Debug, Parser)]
#[command(name = "wauther", version, about)]
pub struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Save a key to the vault
    Add(AddArgs),
    /// Show a key's codes without saving it (always opens the TUI)
    Temp(TempArgs),
    /// Open the app
    Run(RunArgs),
    /// Show where the vault file lives, or move it
    Path {
        /// The new vault file. Without it, the current one is shown.
        path: Option<PathBuf>,
    },
}

#[derive(Debug, Args)]
struct AddArgs {
    /// Name of the key
    #[arg(value_name = "NAME")]
    name_arg: Option<String>,
    /// The secret, or an otpauth:// URI
    #[arg(value_name = "KEY")]
    key_arg: Option<String>,
    /// Name of the key
    #[arg(short, long, value_name = "NAME")]
    name: Option<String>,
    /// The secret, or an otpauth:// URI
    #[arg(short, long, value_name = "KEY")]
    key: Option<String>,
    /// Take the key from an otpauth:// URI. The name comes from the URI
    /// unless you give one.
    #[arg(
        short = 'u',
        long = "from-uri",
        value_name = "URI",
        conflicts_with_all = ["key_arg", "key"]
    )]
    uri: Option<String>,
}

#[derive(Debug, Args)]
struct TempArgs {
    /// The secret, or an otpauth:// URI
    #[arg(
        value_name = "KEY",
        required_unless_present = "uri",
        conflicts_with = "uri"
    )]
    key: Option<String>,
    /// Take the key from an otpauth:// URI
    #[arg(short = 'u', long = "from-uri", value_name = "URI")]
    uri: Option<String>,
}

#[derive(Debug, Args)]
struct RunArgs {
    /// Run in the terminal instead of opening a window
    #[arg(short, long)]
    tui: bool,
}

/// What `main` does once the command has been understood.
#[derive(Debug)]
pub enum Next {
    /// The command did its job and printed what it had to say.
    Done,
    /// Open the terminal UI.
    Tui(Start),
    /// Open the desktop window.
    Gui,
}

/// What an `add` command asks for, with its arguments sorted out.
#[derive(Debug, PartialEq, Eq)]
enum Add {
    Key { name: String, key: String },
    Uri { uri: String, name: Option<String> },
}

impl AddArgs {
    fn resolve(self) -> Result<Add, String> {
        let name = pick(self.name_arg, self.name, "name")?;
        if let Some(uri) = self.uri {
            return Ok(Add::Uri { uri, name });
        }
        let key = pick(self.key_arg, self.key, "key")?;
        match (name, key) {
            (Some(name), Some(key)) => Ok(Add::Key { name, key }),
            (None, _) => Err("missing the name: use `add <NAME> <KEY>` or `--name`".to_owned()),
            (_, None) => Err("missing the key: use `add <NAME> <KEY>` or `--key`".to_owned()),
        }
    }
}

/// A value that may be given as a positional argument or as a flag, but not both.
fn pick(
    positional: Option<String>,
    flag: Option<String>,
    what: &str,
) -> Result<Option<String>, String> {
    match (positional, flag) {
        (Some(_), Some(_)) => Err(format!("the {what} was given twice")),
        (positional, flag) => Ok(positional.or(flag)),
    }
}

/// Does what `cli` asks. Anything that needs a screen is handed back as [`Next`].
///
/// # Errors
///
/// Whatever went wrong, ready to print.
pub fn run(cli: Cli) -> Result<Next, Box<dyn Error>> {
    match cli.command {
        None | Some(Command::Run(RunArgs { tui: false })) => Ok(Next::Gui),
        Some(Command::Run(RunArgs { tui: true })) => {
            Ok(Next::Tui(Start::Vault(Vault::open_default()?)))
        }
        Some(Command::Add(args)) => {
            let mut vault = Vault::open_default()?;
            let name = match args.resolve()? {
                Add::Key { name, key } => {
                    vault.add(&name, &key)?;
                    name.trim().to_owned()
                }
                Add::Uri { uri, name } => vault.add_uri(&uri, name.as_deref())?,
            };
            println!("Added \"{name}\".");
            Ok(Next::Done)
        }
        Some(Command::Temp(args)) => Ok(Next::Tui(Start::Temp(temp_key(args)?))),
        Some(Command::Path { path }) => {
            let mut settings = Settings::load()?;
            match path {
                Some(path) => {
                    let chosen = settings.set_vault_path(&path, &Settings::file()?)?;
                    println!("The vault is now {}", chosen.display());
                    if !chosen.exists() {
                        println!(
                            "It doesn't exist yet. Your keys aren't moved: copy the old file there if you want them."
                        );
                    }
                }
                None => println!("{}", settings.vault_path()?.display()),
            }
            Ok(Next::Done)
        }
    }
}

fn temp_key(args: TempArgs) -> Result<Temp, Box<dyn Error>> {
    match (args.uri, args.key) {
        (Some(uri), _) => {
            let Uri { name, key } = Uri::parse(&uri)?;
            Ok(Temp::new(name, key))
        }
        (None, Some(key)) => Ok(Temp::new(None, Key::parse(&key)?)),
        (None, None) => Err("give a key, or --from-uri".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("wauther").chain(args.iter().copied()))
    }

    fn add(args: &[&str]) -> Result<Add, String> {
        match parse(args).map_err(|e| e.to_string())?.command {
            Some(Command::Add(add)) => add.resolve(),
            other => Err(format!("not an add command: {other:?}")),
        }
    }

    fn key(name: &str, key: &str) -> Add {
        Add::Key {
            name: name.to_owned(),
            key: key.to_owned(),
        }
    }

    #[test]
    fn add_takes_a_name_and_a_key() {
        assert_eq!(
            add(&["add", "github", "GEZDGNBV"]),
            Ok(key("github", "GEZDGNBV"))
        );
    }

    #[test]
    fn add_takes_the_name_and_key_as_flags() {
        assert_eq!(
            add(&["add", "--key", "GEZDGNBV", "--name", "github"]),
            Ok(key("github", "GEZDGNBV"))
        );
    }

    #[test]
    fn add_takes_the_short_flags() {
        assert_eq!(
            add(&["add", "-k", "GEZDGNBV", "-n", "github"]),
            Ok(key("github", "GEZDGNBV"))
        );
    }

    #[test]
    fn add_lets_a_positional_name_go_with_a_key_flag() {
        assert_eq!(
            add(&["add", "github", "-k", "GEZDGNBV"]),
            Ok(key("github", "GEZDGNBV"))
        );
    }

    #[test]
    fn add_takes_a_uri_and_no_name() {
        assert_eq!(
            add(&["add", "--from-uri", "otpauth://totp/a?secret=GEZDGNBV"]),
            Ok(Add::Uri {
                uri: "otpauth://totp/a?secret=GEZDGNBV".to_owned(),
                name: None
            })
        );
    }

    #[test]
    fn add_takes_a_uri_with_a_name() {
        assert_eq!(
            add(&["add", "-u", "otpauth://x", "-n", "mine"]),
            Ok(Add::Uri {
                uri: "otpauth://x".to_owned(),
                name: Some("mine".to_owned())
            })
        );
    }

    #[test]
    fn add_takes_a_uri_with_a_positional_name() {
        assert_eq!(
            add(&["add", "mine", "-u", "otpauth://x"]),
            Ok(Add::Uri {
                uri: "otpauth://x".to_owned(),
                name: Some("mine".to_owned())
            })
        );
    }

    #[test]
    fn add_refuses_a_uri_and_a_key() {
        assert!(parse(&["add", "-u", "otpauth://x", "-k", "GEZDGNBV"]).is_err());
        assert!(parse(&["add", "-u", "otpauth://x", "a", "GEZDGNBV"]).is_err());
    }

    #[test]
    fn add_without_a_key_is_an_error() {
        assert!(add(&["add", "github"]).is_err());
    }

    #[test]
    fn add_without_a_name_is_an_error() {
        assert!(add(&["add", "-k", "GEZDGNBV"]).is_err());
    }

    #[test]
    fn add_refuses_a_name_given_twice() {
        assert!(add(&["add", "a", "GEZDGNBV", "-n", "b"]).is_err());
    }

    #[test]
    fn temp_takes_a_key() {
        let cli = parse(&["temp", "GEZDGNBV"]).unwrap();

        assert!(matches!(
            cli.command,
            Some(Command::Temp(TempArgs {
                key: Some(_),
                uri: None
            }))
        ));
    }

    #[test]
    fn temp_takes_a_uri() {
        let cli = parse(&["temp", "--from-uri", "otpauth://x"]).unwrap();

        assert!(matches!(
            cli.command,
            Some(Command::Temp(TempArgs {
                key: None,
                uri: Some(_)
            }))
        ));
    }

    #[test]
    fn temp_needs_a_key_or_a_uri() {
        assert!(parse(&["temp"]).is_err());
    }

    #[test]
    fn temp_refuses_both() {
        assert!(parse(&["temp", "GEZDGNBV", "-u", "otpauth://x"]).is_err());
    }

    #[test]
    fn temp_names_its_key_after_the_uri() {
        let args = TempArgs {
            key: None,
            uri: Some("otpauth://totp/Example:alice?secret=GEZDGNBV".to_owned()),
        };

        let temp = temp_key(args).unwrap();

        assert_eq!(temp.name, "Example:alice");
    }

    #[test]
    fn temp_with_a_bad_key_is_an_error() {
        let args = TempArgs {
            key: Some("not base32!".to_owned()),
            uri: None,
        };

        assert!(temp_key(args).is_err());
    }

    #[test]
    fn run_has_a_tui_flag() {
        for flag in ["--tui", "-t"] {
            let cli = parse(&["run", flag]).unwrap();

            assert!(matches!(
                cli.command,
                Some(Command::Run(RunArgs { tui: true }))
            ));
        }
    }

    #[test]
    fn run_opens_the_window_by_default() {
        let cli = parse(&["run"]).unwrap();

        assert!(matches!(
            cli.command,
            Some(Command::Run(RunArgs { tui: false }))
        ));
    }

    #[test]
    fn no_command_is_allowed() {
        assert!(parse(&[]).unwrap().command.is_none());
    }

    #[test]
    fn path_takes_an_optional_path() {
        let set = parse(&["path", "/tmp/keys.json"]).unwrap();
        let show = parse(&["path"]).unwrap();

        assert!(matches!(set.command, Some(Command::Path { path: Some(_) })));
        assert!(matches!(show.command, Some(Command::Path { path: None })));
    }
}
