use std::error::Error;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::backend::{Settings, Vault};

#[derive(Debug, Parser)]
#[command(name = "wauther", version, about = "A TOTP authenticator")]
pub struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    #[command(about = "Open the app (the default)")]
    Run,
    #[command(about = "Save a key to the vault")]
    Add(AddArgs),
    #[command(about = "Show where the vault file lives, or move it")]
    Path {
        #[arg(help = "The new vault file; omit to show the current one")]
        path: Option<PathBuf>,
    },
}

#[derive(Debug, Args)]
struct AddArgs {
    #[arg(value_name = "NAME", help = "Name of the key")]
    name_arg: Option<String>,
    #[arg(value_name = "KEY", help = "The secret, or an otpauth:// URI")]
    key_arg: Option<String>,
    #[arg(short, long, value_name = "NAME", help = "Name of the key")]
    name: Option<String>,
    #[arg(
        short,
        long,
        value_name = "KEY",
        help = "The secret, or an otpauth:// URI"
    )]
    key: Option<String>,
    #[arg(
        help = "Take the key from an otpauth:// URI; the name comes from it unless given",
        short = 'u',
        long = "from-uri",
        value_name = "URI",
        conflicts_with_all = ["key_arg", "key"]
    )]
    uri: Option<String>,
}

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

// a value given as a positional or as a flag, not both
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

impl Cli {
    // the vault to open the window with, or None when the command was already handled
    pub fn run(self) -> Result<Option<Vault>, Box<dyn Error>> {
        match self.command {
            None | Some(Command::Run) => Ok(Some(Vault::open_default()?)),
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
                Ok(None)
            }
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
                Ok(None)
            }
        }
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
    fn run_takes_no_arguments() {
        assert!(matches!(
            parse(&["run"]).unwrap().command,
            Some(Command::Run)
        ));
        assert!(parse(&["run", "--tui"]).is_err());
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
