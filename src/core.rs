//! The heart of wauther: TOTP keys, the codes they make, and the vault file
//! that holds them. No drawing and no argument parsing live here, so the CLI,
//! the TUI and the GUI all share it.
//!
//! The vault is one JSON object, `{ "<name>": "<key>", ... }`. A key is
//! normally a plain base32 secret. A key that needs non-default settings
//! (8 digits, a 60 s period, SHA-256 and so on) is kept as the full
//! `otpauth://` URI instead, so its codes stay right.

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use hmac::digest::KeyInit;
use hmac::{Hmac, Mac};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sha2::{Sha256, Sha512};

/// Digits in a code, unless a key says otherwise.
pub const DEFAULT_DIGITS: u32 = 6;
/// Seconds a code lives, unless a key says otherwise.
pub const DEFAULT_PERIOD: u64 = 30;

/// Everything that can go wrong.
#[derive(Debug)]
pub enum Error {
    /// The secret isn't valid base32, or is empty.
    InvalidSecret,
    /// A key setting is out of range (digits `1..=9`, period above `0`).
    InvalidSettings(&'static str),
    /// The `otpauth://` URI can't be used. The text says why.
    InvalidUri(String),
    /// A key needs a name, and got an empty one.
    EmptyName,
    /// A key with this name is already in the vault.
    Duplicate(String),
    /// No key has this name.
    NotFound(String),
    /// There is nowhere to keep data on this system.
    NoDataDir,
    /// A file couldn't be read or written.
    Io {
        /// The file in question.
        path: PathBuf,
        /// What the system said.
        source: io::Error,
    },
    /// A file isn't the JSON we expect.
    Json {
        /// The file in question.
        path: PathBuf,
        /// What the parser said.
        source: serde_json::Error,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSecret => write!(f, "the key is not a valid base32 secret"),
            Self::InvalidSettings(why) => write!(f, "invalid key settings: {why}"),
            Self::InvalidUri(why) => write!(f, "invalid otpauth URI: {why}"),
            Self::EmptyName => write!(f, "the name can't be empty"),
            Self::Duplicate(name) => write!(f, "a key named \"{name}\" already exists"),
            Self::NotFound(name) => write!(f, "no key named \"{name}\""),
            Self::NoDataDir => write!(f, "can't find a place to keep data on this system"),
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Json { path, source } => write!(f, "{}: {source}", path.display()),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Json { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// The hash a key's codes are made with.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Algorithm {
    /// SHA-1. What nearly every service uses.
    #[default]
    Sha1,
    /// SHA-256.
    Sha256,
    /// SHA-512.
    Sha512,
}

impl Algorithm {
    /// The name `otpauth://` URIs use.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sha1 => "SHA1",
            Self::Sha256 => "SHA256",
            Self::Sha512 => "SHA512",
        }
    }
}

impl FromStr for Algorithm {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_uppercase().as_str() {
            "SHA1" => Ok(Self::Sha1),
            "SHA256" => Ok(Self::Sha256),
            "SHA512" => Ok(Self::Sha512),
            other => Err(Error::InvalidUri(format!("unknown algorithm \"{other}\""))),
        }
    }
}

/// A secret, plus the settings its codes are made with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    secret: Vec<u8>,
    algorithm: Algorithm,
    digits: u32,
    period: u64,
}

impl Key {
    /// A key from raw secret bytes and explicit settings.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidSecret`] for an empty secret, [`Error::InvalidSettings`]
    /// for `digits` outside `1..=9` or a zero `period`.
    pub fn new(
        secret: Vec<u8>,
        algorithm: Algorithm,
        digits: u32,
        period: u64,
    ) -> Result<Self, Error> {
        if secret.is_empty() {
            return Err(Error::InvalidSecret);
        }
        if !(1..=9).contains(&digits) {
            return Err(Error::InvalidSettings("digits must be 1 to 9"));
        }
        if period == 0 {
            return Err(Error::InvalidSettings("period must be above 0"));
        }
        Ok(Self {
            secret,
            algorithm,
            digits,
            period,
        })
    }

    /// A key from what a person types or stores: a base32 secret (spaces,
    /// dashes, `=` padding and lowercase are fine) or an `otpauth://` URI.
    ///
    /// # Errors
    ///
    /// See [`Error`].
    pub fn parse(input: &str) -> Result<Self, Error> {
        let input = input.trim();
        if is_uri(input) {
            return Ok(Uri::parse(input)?.key);
        }
        let secret = base32_decode(input).ok_or(Error::InvalidSecret)?;
        Self::new(secret, Algorithm::Sha1, DEFAULT_DIGITS, DEFAULT_PERIOD)
    }

    /// The code at `unix` seconds since the epoch.
    #[must_use]
    pub fn code_at(&self, unix: u64) -> String {
        let code = self.hotp(unix / self.period);
        format!("{code:0width$}", width = self.digits as usize)
    }

    /// The code right now.
    #[must_use]
    pub fn code(&self) -> String {
        self.code_at(now())
    }

    /// Seconds left before the code at `unix` changes, from `period` down to 1.
    #[must_use]
    pub const fn remaining_at(&self, unix: u64) -> u64 {
        self.period - unix % self.period
    }

    /// Seconds a code lives.
    #[must_use]
    pub const fn period(&self) -> u64 {
        self.period
    }

    /// Digits in a code.
    #[must_use]
    pub const fn digits(&self) -> u32 {
        self.digits
    }

    /// The text to keep in the vault: the bare base32 secret when every
    /// setting is a default, otherwise a full `otpauth://` URI.
    #[must_use]
    pub fn to_stored(&self) -> String {
        let secret = base32_encode(&self.secret);
        if self.algorithm == Algorithm::Sha1
            && self.digits == DEFAULT_DIGITS
            && self.period == DEFAULT_PERIOD
        {
            return secret;
        }
        format!(
            "otpauth://totp/wauther?secret={secret}&algorithm={}&digits={}&period={}",
            self.algorithm.as_str(),
            self.digits,
            self.period,
        )
    }

    /// RFC 4226: the dynamically truncated HMAC of `counter`, as a number.
    fn hotp(&self, counter: u64) -> u32 {
        let message = counter.to_be_bytes();
        let digest = match self.algorithm {
            Algorithm::Sha1 => hmac_bytes::<Hmac<Sha1>>(&self.secret, &message),
            Algorithm::Sha256 => hmac_bytes::<Hmac<Sha256>>(&self.secret, &message),
            Algorithm::Sha512 => hmac_bytes::<Hmac<Sha512>>(&self.secret, &message),
        };
        let offset = usize::from(digest[digest.len() - 1] & 0x0f);
        let truncated = u32::from_be_bytes([
            digest[offset],
            digest[offset + 1],
            digest[offset + 2],
            digest[offset + 3],
        ]);
        (truncated & 0x7fff_ffff) % 10_u32.pow(self.digits)
    }
}

fn hmac_bytes<M: Mac + KeyInit>(key: &[u8], message: &[u8]) -> Vec<u8> {
    let mut mac = <M as Mac>::new_from_slice(key).expect("HMAC accepts keys of any length");
    mac.update(message);
    mac.finalize().into_bytes().to_vec()
}

/// Seconds since the Unix epoch.
#[must_use]
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

// ============================================================================
// otpauth:// URIs
// ============================================================================

/// What an `otpauth://totp/...` URI holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uri {
    /// A name made from the URI's label and issuer, if it had either.
    pub name: Option<String>,
    /// The key, with the URI's algorithm, digits and period.
    pub key: Key,
}

fn is_uri(input: &str) -> bool {
    input
        .get(..10)
        .is_some_and(|start| start.eq_ignore_ascii_case("otpauth://"))
}

impl Uri {
    /// Reads an `otpauth://totp/<label>?secret=...` URI.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidUri`] if it isn't a TOTP URI or lacks a secret, and
    /// the key errors of [`Key::new`].
    pub fn parse(uri: &str) -> Result<Self, Error> {
        let bad = |why: &str| Error::InvalidUri(why.to_owned());

        let uri = uri.trim();
        if !is_uri(uri) {
            return Err(bad("it doesn't start with otpauth://"));
        }
        let rest = &uri[10..];
        let (kind, rest) = rest.split_once('/').ok_or_else(|| bad("no label"))?;
        if !kind.eq_ignore_ascii_case("totp") {
            return Err(bad("only totp keys are supported"));
        }
        let (label, query) = rest.split_once('?').unwrap_or((rest, ""));
        let label = percent_decode(label).ok_or_else(|| bad("bad % escape in the label"))?;

        let (mut secret, mut issuer) = (None, None);
        let (mut algorithm, mut digits, mut period) =
            (Algorithm::Sha1, DEFAULT_DIGITS, DEFAULT_PERIOD);
        for pair in query.split('&').filter(|pair| !pair.is_empty()) {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            let value = percent_decode(value).ok_or_else(|| bad("bad % escape"))?;
            match name.to_ascii_lowercase().as_str() {
                "secret" => secret = Some(value),
                "issuer" => issuer = Some(value),
                "algorithm" => algorithm = value.parse()?,
                "digits" => digits = value.parse().map_err(|_| bad("digits isn't a number"))?,
                "period" => period = value.parse().map_err(|_| bad("period isn't a number"))?,
                _ => {}
            }
        }

        let secret = secret.ok_or_else(|| bad("no secret"))?;
        let secret = base32_decode(&secret).ok_or(Error::InvalidSecret)?;
        let key = Key::new(secret, algorithm, digits, period)?;

        let label = Some(label.trim().to_owned()).filter(|label| !label.is_empty());
        let issuer = issuer
            .map(|issuer| issuer.trim().to_owned())
            .filter(|issuer| !issuer.is_empty());
        let name = match (label, issuer) {
            (Some(label), Some(issuer)) if !label.contains(':') => {
                Some(format!("{issuer}: {label}"))
            }
            (Some(label), _) => Some(label),
            (None, issuer) => issuer,
        };

        Ok(Self { name, key })
    }
}

/// Decodes `%XX` escapes and `+` as a space. `None` for a broken escape or
/// bytes that aren't UTF-8.
fn percent_decode(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let hex = input.get(i + 1..i + 3)?;
                out.push(u8::from_str_radix(hex, 16).ok()?);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

// ============================================================================
// base32 (RFC 4648)
// ============================================================================

const BASE32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// Decodes base32, ignoring case, whitespace, `-` and `=`. `None` for any
/// other character, or for nothing at all.
fn base32_decode(input: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() * 5 / 8);
    let (mut buffer, mut bits) = (0_u32, 0_u32);
    for c in input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-' && *c != '=')
    {
        let value = BASE32
            .iter()
            .position(|&b| char::from(b) == c.to_ascii_uppercase())?;
        buffer = (buffer << 5) | u32::try_from(value).ok()?;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from(buffer >> bits).ok()?);
            buffer &= (1 << bits) - 1;
        }
    }
    (!out.is_empty()).then_some(out)
}

/// Encodes base32 without padding.
fn base32_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len() * 8 / 5 + 1);
    let (mut buffer, mut bits) = (0_u32, 0_u32);
    for &byte in data {
        buffer = (buffer << 8) | u32::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(char::from(BASE32[(buffer >> bits) as usize & 31]));
        }
        buffer &= (1 << bits) - 1;
    }
    if bits > 0 {
        out.push(char::from(BASE32[(buffer << (5 - bits)) as usize & 31]));
    }
    out
}

// ============================================================================
// The vault
// ============================================================================

/// One line of the vault, with its code worked out for a moment in time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The key's name.
    pub name: String,
    /// The code, or `None` if the stored key can't be read.
    pub code: Option<String>,
    /// Seconds until `code` changes.
    pub remaining: u64,
    /// Seconds a code lives. Handy for drawing a countdown.
    pub period: u64,
}

/// The keys in the vault file, in the order they were added.
///
/// Every change is written to disk straight away, so there is nothing to
/// forget to save. If the write fails, the change is undone.
#[derive(Debug, Clone)]
pub struct Vault {
    path: PathBuf,
    entries: IndexMap<String, String>,
}

impl Vault {
    /// Opens the vault at `path`. A file that isn't there yet is an empty vault.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] if the file can't be read, [`Error::Json`] if it isn't a
    /// JSON object of strings. Such a file is never overwritten.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, Error> {
        let path = path.into();
        let entries = match fs::read_to_string(&path) {
            Ok(text) if text.trim().is_empty() => IndexMap::new(),
            Ok(text) => serde_json::from_str(&text).map_err(|source| Error::Json {
                path: path.clone(),
                source,
            })?,
            Err(source) if source.kind() == io::ErrorKind::NotFound => IndexMap::new(),
            Err(source) => return Err(Error::Io { path, source }),
        };
        Ok(Self { path, entries })
    }

    /// Opens the vault the settings point at. See [`Settings`].
    ///
    /// # Errors
    ///
    /// See [`Settings::load`] and [`Vault::open`].
    pub fn open_default() -> Result<Self, Error> {
        Self::open(Settings::load()?.vault_path()?)
    }

    /// Where the vault file lives.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// How many keys there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there are no keys.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The names, in the order they were added.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    /// Whether a key has this name.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.entries.contains_key(name)
    }

    /// The key called `name`.
    ///
    /// # Errors
    ///
    /// [`Error::NotFound`], or a key error if the stored text is unusable.
    pub fn key(&self, name: &str) -> Result<Key, Error> {
        let stored = self
            .entries
            .get(name)
            .ok_or_else(|| Error::NotFound(name.to_owned()))?;
        Key::parse(stored)
    }

    /// Every key with its code at `unix` seconds. Pass [`now`] for the present.
    #[must_use]
    pub fn rows(&self, unix: u64) -> Vec<Row> {
        self.entries
            .iter()
            .map(|(name, stored)| match Key::parse(stored) {
                Ok(key) => Row {
                    name: name.clone(),
                    code: Some(key.code_at(unix)),
                    remaining: key.remaining_at(unix),
                    period: key.period(),
                },
                Err(_) => Row {
                    name: name.clone(),
                    code: None,
                    remaining: 0,
                    period: DEFAULT_PERIOD,
                },
            })
            .collect()
    }

    /// Adds a key. `key` is a base32 secret or an `otpauth://` URI.
    ///
    /// # Errors
    ///
    /// [`Error::EmptyName`], [`Error::Duplicate`], a key error, or the error
    /// from saving.
    pub fn add(&mut self, name: &str, key: &str) -> Result<(), Error> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::EmptyName);
        }
        if self.contains(name) {
            return Err(Error::Duplicate(name.to_owned()));
        }
        let stored = Key::parse(key)?.to_stored();
        self.change(|entries| {
            entries.insert(name.to_owned(), stored);
            Ok(())
        })
    }

    /// Adds the key in an `otpauth://` URI, named `name`, or else after the
    /// URI's own label. Gives back the name it used.
    ///
    /// # Errors
    ///
    /// [`Error::EmptyName`] if neither `name` nor the URI has one, otherwise
    /// as [`Vault::add`].
    pub fn add_uri(&mut self, uri: &str, name: Option<&str>) -> Result<String, Error> {
        let parsed = Uri::parse(uri)?;
        let name = name
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .or(parsed.name)
            .ok_or(Error::EmptyName)?;
        self.add(&name, &parsed.key.to_stored())?;
        Ok(name)
    }

    /// Removes the key called `name`.
    ///
    /// # Errors
    ///
    /// [`Error::NotFound`], or the error from saving.
    pub fn remove(&mut self, name: &str) -> Result<(), Error> {
        self.remove_many([name])
    }

    /// Removes several keys with a single write. If any name is missing,
    /// nothing is removed.
    ///
    /// # Errors
    ///
    /// [`Error::NotFound`] for the first missing name, or the error from saving.
    pub fn remove_many<'a>(
        &mut self,
        names: impl IntoIterator<Item = &'a str>,
    ) -> Result<(), Error> {
        let names: Vec<&str> = names.into_iter().collect();
        self.change(|entries| {
            for name in names {
                entries
                    .shift_remove(name)
                    .ok_or_else(|| Error::NotFound(name.to_owned()))?;
            }
            Ok(())
        })
    }

    /// Runs `edit` on the entries and saves. If either fails, the entries go
    /// back to how they were.
    fn change(
        &mut self,
        edit: impl FnOnce(&mut IndexMap<String, String>) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let before = self.entries.clone();
        let result = edit(&mut self.entries).and_then(|()| self.save());
        if result.is_err() {
            self.entries = before;
        }
        result
    }

    fn save(&self) -> Result<(), Error> {
        let json = serde_json::to_string_pretty(&self.entries).map_err(|source| Error::Json {
            path: self.path.clone(),
            source,
        })?;
        write_private(&self.path, json.as_bytes()).map_err(|source| Error::Io {
            path: self.path.clone(),
            source,
        })
    }
}

/// Writes `contents` to `path` through a temporary file, so a crash can't
/// leave half a vault. On Unix only the owner can read the file.
fn write_private(path: &Path, contents: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        fs::create_dir_all(dir)?;
    }
    let temporary = path.with_extension("tmp");

    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    file.write_all(contents)?;
    file.sync_all()?;
    drop(file);

    fs::rename(&temporary, path)
}

// ============================================================================
// Settings
// ============================================================================

/// Settings kept between runs, in `config.json` in the user's config folder.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// Where the vault file lives. `None` means the default place.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path: Option<PathBuf>,
}

impl Settings {
    /// Where the settings file is.
    ///
    /// # Errors
    ///
    /// [`Error::NoDataDir`] if the system has no config folder.
    pub fn file() -> Result<PathBuf, Error> {
        dirs::config_dir()
            .map(|dir| dir.join("wauther").join("config.json"))
            .ok_or(Error::NoDataDir)
    }

    /// Reads the settings. A missing file gives the defaults.
    ///
    /// # Errors
    ///
    /// See [`Settings::load_from`].
    pub fn load() -> Result<Self, Error> {
        Self::load_from(&Self::file()?)
    }

    /// Reads settings from `file`. A missing file gives the defaults.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] or [`Error::Json`].
    pub fn load_from(file: &Path) -> Result<Self, Error> {
        match fs::read_to_string(file) {
            Ok(text) if text.trim().is_empty() => Ok(Self::default()),
            Ok(text) => serde_json::from_str(&text).map_err(|source| Error::Json {
                path: file.to_owned(),
                source,
            }),
            Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(Error::Io {
                path: file.to_owned(),
                source,
            }),
        }
    }

    /// Writes the settings to `file`.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] or [`Error::Json`].
    pub fn save_to(&self, file: &Path) -> Result<(), Error> {
        let json = serde_json::to_string_pretty(self).map_err(|source| Error::Json {
            path: file.to_owned(),
            source,
        })?;
        write_private(file, json.as_bytes()).map_err(|source| Error::Io {
            path: file.to_owned(),
            source,
        })
    }

    /// Where the vault file lives: the chosen path, or `keys.json` in the
    /// user's data folder.
    ///
    /// # Errors
    ///
    /// [`Error::NoDataDir`] if none is chosen and the system has no data folder.
    pub fn vault_path(&self) -> Result<PathBuf, Error> {
        match &self.path {
            Some(path) => Ok(path.clone()),
            None => dirs::data_dir()
                .map(|dir| dir.join("wauther").join("keys.json"))
                .ok_or(Error::NoDataDir),
        }
    }

    /// Chooses where the vault file lives, and saves the choice to `file`.
    /// A relative `path` is made absolute first. The old vault is not moved.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] or [`Error::Json`].
    pub fn set_vault_path(&mut self, path: &Path, file: &Path) -> Result<PathBuf, Error> {
        let path = std::path::absolute(path).map_err(|source| Error::Io {
            path: path.to_owned(),
            source,
        })?;
        self.path = Some(path.clone());
        self.save_to(file)?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The RFC 6238 test secrets, which are plain ASCII.
    const SHA1_SECRET: &[u8] = b"12345678901234567890";
    const SHA256_SECRET: &[u8] = b"12345678901234567890123456789012";
    const SHA512_SECRET: &[u8] =
        b"1234567890123456789012345678901234567890123456789012345678901234";

    fn rfc_key(secret: &[u8], algorithm: Algorithm) -> Key {
        Key::new(secret.to_vec(), algorithm, 8, 30).expect("valid test key")
    }

    // ---- codes -------------------------------------------------------------

    #[test]
    fn sha1_matches_the_rfc_6238_vectors() {
        let key = rfc_key(SHA1_SECRET, Algorithm::Sha1);

        assert_eq!(key.code_at(59), "94287082");
        assert_eq!(key.code_at(1_111_111_109), "07081804");
        assert_eq!(key.code_at(1_111_111_111), "14050471");
        assert_eq!(key.code_at(1_234_567_890), "89005924");
        assert_eq!(key.code_at(2_000_000_000), "69279037");
        assert_eq!(key.code_at(20_000_000_000), "65353130");
    }

    #[test]
    fn sha256_matches_the_rfc_6238_vectors() {
        let key = rfc_key(SHA256_SECRET, Algorithm::Sha256);

        assert_eq!(key.code_at(59), "46119246");
        assert_eq!(key.code_at(1_111_111_109), "68084774");
    }

    #[test]
    fn sha512_matches_the_rfc_6238_vectors() {
        let key = rfc_key(SHA512_SECRET, Algorithm::Sha512);

        assert_eq!(key.code_at(59), "90693936");
        assert_eq!(key.code_at(1_111_111_109), "25091201");
    }

    #[test]
    fn a_code_is_the_same_for_the_whole_period() {
        let key = Key::parse("GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ").unwrap();

        assert_eq!(key.code_at(60), key.code_at(89));
    }

    #[test]
    fn a_code_changes_at_the_next_period() {
        let key = Key::parse("GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ").unwrap();

        assert_ne!(key.code_at(89), key.code_at(90));
    }

    #[test]
    fn a_code_keeps_its_leading_zeros() {
        // At T=1111111109 the 8-digit SHA-1 code is 07081804.
        let key = rfc_key(SHA1_SECRET, Algorithm::Sha1);

        assert!(key.code_at(1_111_111_109).starts_with('0'));
    }

    #[test]
    fn the_default_key_makes_six_digits() {
        let key = Key::parse("GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ").unwrap();

        assert_eq!(key.code_at(59).len(), 6);
    }

    #[test]
    fn remaining_counts_down_from_the_period() {
        let key = Key::parse("GEZDGNBVGY3TQOJQ").unwrap();

        assert_eq!(key.remaining_at(60), 30);
        assert_eq!(key.remaining_at(89), 1);
    }

    // ---- keys --------------------------------------------------------------

    #[test]
    fn a_key_ignores_case_spaces_dashes_and_padding() {
        let plain = Key::parse("GEZDGNBVGY3TQOJQ").unwrap();

        assert_eq!(Key::parse("gezd gnbv-gy3t qojq====").unwrap(), plain);
    }

    #[test]
    fn a_key_rejects_characters_outside_base32() {
        assert!(matches!(Key::parse("GEZDG1BV"), Err(Error::InvalidSecret)));
    }

    #[test]
    fn an_empty_key_is_rejected() {
        assert!(matches!(Key::parse("  "), Err(Error::InvalidSecret)));
    }

    #[test]
    fn digits_outside_one_to_nine_are_rejected() {
        let key = Key::new(vec![1], Algorithm::Sha1, 10, 30);

        assert!(matches!(key, Err(Error::InvalidSettings(_))));
    }

    #[test]
    fn a_zero_period_is_rejected() {
        let key = Key::new(vec![1], Algorithm::Sha1, 6, 0);

        assert!(matches!(key, Err(Error::InvalidSettings(_))));
    }

    #[test]
    fn a_default_key_is_stored_as_its_bare_secret() {
        let key = Key::parse("gezd gnbv gy3t qojq").unwrap();

        assert_eq!(key.to_stored(), "GEZDGNBVGY3TQOJQ");
    }

    #[test]
    fn a_custom_key_is_stored_as_a_uri_and_reads_back_the_same() {
        let key = Key::new(b"secret!".to_vec(), Algorithm::Sha256, 8, 60).unwrap();

        assert_eq!(Key::parse(&key.to_stored()).unwrap(), key);
    }

    // ---- base32 ------------------------------------------------------------

    #[test]
    fn base32_encodes_the_rfc_4648_vectors() {
        assert_eq!(base32_encode(b"f"), "MY");
        assert_eq!(base32_encode(b"fo"), "MZXQ");
        assert_eq!(base32_encode(b"foo"), "MZXW6");
        assert_eq!(base32_encode(b"foob"), "MZXW6YQ");
        assert_eq!(base32_encode(b"fooba"), "MZXW6YTB");
        assert_eq!(base32_encode(b"foobar"), "MZXW6YTBOI");
    }

    #[test]
    fn base32_decodes_the_rfc_4648_vectors() {
        assert_eq!(base32_decode("MY======").unwrap(), b"f");
        assert_eq!(base32_decode("MZXW6YQ=").unwrap(), b"foob");
        assert_eq!(base32_decode("MZXW6YTBOI======").unwrap(), b"foobar");
    }

    #[test]
    fn base32_round_trips_every_length() {
        for length in 1..=40_u8 {
            let data: Vec<u8> = (0..length).map(|i| i.wrapping_mul(37)).collect();

            assert_eq!(base32_decode(&base32_encode(&data)).unwrap(), data);
        }
    }

    // ---- URIs --------------------------------------------------------------

    #[test]
    fn a_uri_gives_its_secret_and_label() {
        let uri = Uri::parse("otpauth://totp/Alice?secret=GEZDGNBVGY3TQOJQ").unwrap();

        assert_eq!(uri.name.as_deref(), Some("Alice"));
        assert_eq!(uri.key, Key::parse("GEZDGNBVGY3TQOJQ").unwrap());
    }

    #[test]
    fn a_uri_label_is_percent_decoded() {
        let uri = Uri::parse("otpauth://totp/Example%3Aalice%40mail.com?secret=GEZDGNBV").unwrap();

        assert_eq!(uri.name.as_deref(), Some("Example:alice@mail.com"));
    }

    #[test]
    fn a_uri_issuer_is_put_before_a_label_without_one() {
        let uri = Uri::parse("otpauth://totp/alice?secret=GEZDGNBV&issuer=Example").unwrap();

        assert_eq!(uri.name.as_deref(), Some("Example: alice"));
    }

    #[test]
    fn a_uri_label_that_names_the_issuer_is_left_alone() {
        let uri =
            Uri::parse("otpauth://totp/Example:alice?secret=GEZDGNBV&issuer=Example").unwrap();

        assert_eq!(uri.name.as_deref(), Some("Example:alice"));
    }

    #[test]
    fn a_uri_without_a_label_is_named_after_its_issuer() {
        let uri = Uri::parse("otpauth://totp/?secret=GEZDGNBV&issuer=Example").unwrap();

        assert_eq!(uri.name.as_deref(), Some("Example"));
    }

    #[test]
    fn a_uri_can_set_algorithm_digits_and_period() {
        let uri =
            Uri::parse("otpauth://totp/a?secret=GEZDGNBV&algorithm=sha512&digits=8&period=60")
                .unwrap();

        assert_eq!(uri.key.digits(), 8);
        assert_eq!(uri.key.period(), 60);
        assert_eq!(uri.key.algorithm, Algorithm::Sha512);
    }

    #[test]
    fn a_uri_scheme_is_matched_in_any_case() {
        assert!(Uri::parse("OTPAUTH://TOTP/a?secret=GEZDGNBV").is_ok());
    }

    #[test]
    fn a_hotp_uri_is_rejected() {
        let uri = Uri::parse("otpauth://hotp/a?secret=GEZDGNBV&counter=1");

        assert!(matches!(uri, Err(Error::InvalidUri(_))));
    }

    #[test]
    fn a_uri_without_a_secret_is_rejected() {
        assert!(matches!(
            Uri::parse("otpauth://totp/a?issuer=x"),
            Err(Error::InvalidUri(_))
        ));
    }

    #[test]
    fn a_uri_with_an_unknown_algorithm_is_rejected() {
        let uri = Uri::parse("otpauth://totp/a?secret=GEZDGNBV&algorithm=MD5");

        assert!(matches!(uri, Err(Error::InvalidUri(_))));
    }

    #[test]
    fn something_that_is_not_a_uri_is_rejected() {
        assert!(matches!(
            Uri::parse("https://example.com"),
            Err(Error::InvalidUri(_))
        ));
    }

    #[test]
    fn percent_decoding_rejects_a_broken_escape() {
        assert_eq!(percent_decode("a%2"), None);
        assert_eq!(percent_decode("a%zz"), None);
    }

    #[test]
    fn percent_decoding_turns_plus_into_a_space() {
        assert_eq!(percent_decode("a+b%21").as_deref(), Some("a b!"));
    }

    // ---- vault -------------------------------------------------------------

    fn vault_in(dir: &tempfile::TempDir) -> Vault {
        Vault::open(dir.path().join("keys.json")).unwrap()
    }

    #[test]
    fn a_missing_vault_file_is_an_empty_vault() {
        let dir = tempfile::tempdir().unwrap();

        assert!(vault_in(&dir).is_empty());
    }

    #[test]
    fn an_added_key_is_still_there_after_reopening() {
        let dir = tempfile::tempdir().unwrap();
        vault_in(&dir).add("github", "GEZDGNBV").unwrap();

        let reopened = vault_in(&dir);

        assert_eq!(reopened.names().collect::<Vec<_>>(), ["github"]);
    }

    #[test]
    fn the_vault_file_is_a_plain_json_object_of_names_to_keys() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = vault_in(&dir);
        vault.add("github", "gezd gnbv").unwrap();

        let text = fs::read_to_string(vault.path()).unwrap();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();

        assert_eq!(json["github"], "GEZDGNBV");
    }

    #[test]
    fn keys_stay_in_the_order_they_were_added() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = vault_in(&dir);
        for name in ["zed", "alpha", "mid"] {
            vault.add(name, "GEZDGNBV").unwrap();
        }

        assert_eq!(vault.names().collect::<Vec<_>>(), ["zed", "alpha", "mid"]);
    }

    #[test]
    fn adding_a_name_twice_is_an_error_and_keeps_the_first_key() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = vault_in(&dir);
        vault.add("a", "GEZDGNBV").unwrap();

        let result = vault.add("a", "MZXW6YTB");

        assert!(matches!(result, Err(Error::Duplicate(_))));
        assert_eq!(vault.key("a").unwrap(), Key::parse("GEZDGNBV").unwrap());
    }

    #[test]
    fn a_blank_name_is_rejected() {
        let dir = tempfile::tempdir().unwrap();

        assert!(matches!(
            vault_in(&dir).add("   ", "GEZDGNBV"),
            Err(Error::EmptyName)
        ));
    }

    #[test]
    fn a_bad_key_is_rejected_and_nothing_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = vault_in(&dir);

        assert!(vault.add("a", "not base32!").is_err());
        assert!(vault.is_empty());
        assert!(!vault.path().exists());
    }

    #[test]
    fn a_name_is_trimmed() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = vault_in(&dir);
        vault.add("  github  ", "GEZDGNBV").unwrap();

        assert!(vault.contains("github"));
    }

    #[test]
    fn a_removed_key_is_gone_after_reopening() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = vault_in(&dir);
        vault.add("a", "GEZDGNBV").unwrap();
        vault.add("b", "GEZDGNBV").unwrap();
        vault.remove("a").unwrap();

        assert_eq!(vault_in(&dir).names().collect::<Vec<_>>(), ["b"]);
    }

    #[test]
    fn removing_a_missing_key_is_an_error() {
        let dir = tempfile::tempdir().unwrap();

        assert!(matches!(
            vault_in(&dir).remove("nope"),
            Err(Error::NotFound(_))
        ));
    }

    #[test]
    fn removing_many_removes_nothing_if_one_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = vault_in(&dir);
        vault.add("a", "GEZDGNBV").unwrap();
        vault.add("b", "GEZDGNBV").unwrap();

        let result = vault.remove_many(["a", "nope"]);

        assert!(result.is_err());
        assert_eq!(vault.len(), 2);
    }

    #[test]
    fn removing_many_removes_them_all_with_one_write() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = vault_in(&dir);
        for name in ["a", "b", "c"] {
            vault.add(name, "GEZDGNBV").unwrap();
        }
        vault.remove_many(["a", "c"]).unwrap();

        assert_eq!(vault_in(&dir).names().collect::<Vec<_>>(), ["b"]);
    }

    #[test]
    fn a_failed_save_undoes_the_change() {
        let dir = tempfile::tempdir().unwrap();
        // A directory where the file should go makes the final rename fail.
        let blocked = dir.path().join("keys.json");
        fs::create_dir(&blocked).unwrap();
        let mut vault = Vault {
            path: blocked,
            entries: IndexMap::new(),
        };

        assert!(vault.add("a", "GEZDGNBV").is_err());
        assert!(vault.is_empty());
    }

    #[test]
    fn add_uri_names_the_key_after_the_uri_label() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = vault_in(&dir);

        let name = vault
            .add_uri("otpauth://totp/Example:alice?secret=GEZDGNBV", None)
            .unwrap();

        assert_eq!(name, "Example:alice");
        assert!(vault.contains("Example:alice"));
    }

    #[test]
    fn add_uri_prefers_the_name_it_is_given() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = vault_in(&dir);

        let name = vault
            .add_uri("otpauth://totp/Example:alice?secret=GEZDGNBV", Some("mine"))
            .unwrap();

        assert_eq!(name, "mine");
    }

    #[test]
    fn add_uri_needs_a_name_from_somewhere() {
        let dir = tempfile::tempdir().unwrap();

        let result = vault_in(&dir).add_uri("otpauth://totp/?secret=GEZDGNBV", None);

        assert!(matches!(result, Err(Error::EmptyName)));
    }

    #[test]
    fn add_uri_keeps_non_default_settings() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = vault_in(&dir);
        vault
            .add_uri("otpauth://totp/a?secret=GEZDGNBV&digits=8&period=60", None)
            .unwrap();

        let key = vault.key("a").unwrap();

        assert_eq!((key.digits(), key.period()), (8, 60));
    }

    #[test]
    fn rows_have_a_code_and_a_countdown() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = vault_in(&dir);
        vault.add("a", "GEZDGNBVGY3TQOJQ").unwrap();

        let rows = vault.rows(59);
        let expected = Key::parse("GEZDGNBVGY3TQOJQ").unwrap().code_at(59);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].code.as_deref(), Some(expected.as_str()));
        assert_eq!(rows[0].remaining, 1);
        assert_eq!(rows[0].period, 30);
    }

    #[test]
    fn a_hand_edited_bad_key_is_a_row_without_a_code() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("keys.json");
        fs::write(&path, r#"{"broken": "!!!"}"#).unwrap();

        let rows = Vault::open(&path).unwrap().rows(0);

        assert_eq!(rows[0].code, None);
    }

    #[test]
    fn a_file_that_is_not_json_is_an_error_and_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("keys.json");
        fs::write(&path, "not json").unwrap();

        assert!(matches!(Vault::open(&path), Err(Error::Json { .. })));
        assert_eq!(fs::read_to_string(&path).unwrap(), "not json");
    }

    #[test]
    fn an_empty_file_is_an_empty_vault() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("keys.json");
        fs::write(&path, "").unwrap();

        assert!(Vault::open(&path).unwrap().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn the_vault_file_can_only_be_read_by_its_owner() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let mut vault = vault_in(&dir);
        vault.add("a", "GEZDGNBV").unwrap();

        let mode = fs::metadata(vault.path()).unwrap().permissions().mode();

        assert_eq!(mode & 0o777, 0o600);
    }

    // ---- settings ----------------------------------------------------------

    #[test]
    fn missing_settings_are_the_defaults() {
        let dir = tempfile::tempdir().unwrap();

        let settings = Settings::load_from(&dir.path().join("config.json")).unwrap();

        assert_eq!(settings, Settings::default());
    }

    #[test]
    fn a_chosen_vault_path_survives_reloading() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("config.json");
        let vault = dir.path().join("elsewhere.json");

        Settings::default().set_vault_path(&vault, &file).unwrap();
        let reloaded = Settings::load_from(&file).unwrap();

        assert_eq!(reloaded.vault_path().unwrap(), vault);
    }

    #[test]
    fn a_relative_vault_path_is_saved_as_an_absolute_one() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("config.json");

        let saved = Settings::default()
            .set_vault_path(Path::new("keys.json"), &file)
            .unwrap();

        assert!(saved.is_absolute());
    }
}
