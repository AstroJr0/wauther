use std::cell::RefCell;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use hmac::digest::KeyInit;
use hmac::{Hmac, Mac};
use indexmap::IndexMap;
use qtbridge::{QApp, qobject};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sha2::{Sha256, Sha512};

const DEFAULT_DIGITS: u32 = 6;
const DEFAULT_PERIOD: u64 = 30;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the key is not a valid base32 secret")]
    InvalidSecret,
    #[error("invalid key settings: {0}")]
    InvalidSettings(&'static str),
    #[error("invalid otpauth URI: {0}")]
    InvalidUri(String),
    #[error("the name can't be empty")]
    EmptyName,
    #[error("a key named \"{0}\" already exists")]
    Duplicate(String),
    #[error("no key named \"{0}\"")]
    NotFound(String),
    #[error("can't find a place to keep data on this system")]
    NoDataDir,
    #[error("{}: {source}", .path.display())]
    Io { path: PathBuf, source: io::Error },
    #[error("{}: {source}", .path.display())]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Algorithm {
    #[default]
    Sha1,
    Sha256,
    Sha512,
}

impl Algorithm {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Sha1 => "SHA1",
            Self::Sha256 => "SHA256",
            Self::Sha512 => "SHA512",
        }
    }
}

impl FromStr for Algorithm {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        match s.to_ascii_uppercase().as_str() {
            "SHA1" => Ok(Self::Sha1),
            "SHA256" => Ok(Self::Sha256),
            "SHA512" => Ok(Self::Sha512),
            other => Err(Error::InvalidUri(format!("unknown algorithm \"{other}\""))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Key {
    secret: Vec<u8>,
    algorithm: Algorithm,
    digits: u32,
    period: u64,
}

impl Key {
    fn new(secret: Vec<u8>, algorithm: Algorithm, digits: u32, period: u64) -> Result<Self, Error> {
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

    // a bare base32 secret or a full otpauth:// URI
    fn parse(input: &str) -> Result<Self, Error> {
        let input = input.trim();
        if strip_scheme(input).is_some() {
            return Ok(Uri::parse(input)?.key);
        }
        let secret = base32_decode(input).ok_or(Error::InvalidSecret)?;
        Self::new(secret, Algorithm::Sha1, DEFAULT_DIGITS, DEFAULT_PERIOD)
    }

    fn code_at(&self, unix: u64) -> String {
        let code = self.hotp(unix / self.period);
        format!("{code:0width$}", width = self.digits as usize)
    }

    const fn remaining_at(&self, unix: u64) -> u64 {
        self.period - unix % self.period
    }

    // the bare secret when every setting is the default, so the vault stays readable
    fn to_stored(&self) -> String {
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

    fn hotp(&self, counter: u64) -> u32 {
        let message = counter.to_be_bytes();
        let digest = match self.algorithm {
            Algorithm::Sha1 => hmac_bytes::<Hmac<Sha1>>(&self.secret, &message),
            Algorithm::Sha256 => hmac_bytes::<Hmac<Sha256>>(&self.secret, &message),
            Algorithm::Sha512 => hmac_bytes::<Hmac<Sha512>>(&self.secret, &message),
        };
        let offset = usize::from(digest[digest.len() - 1] & 0x0f);
        let word: [u8; 4] = digest[offset..offset + 4]
            .try_into()
            .expect("the offset is at most 15 and every digest is longer than 19 bytes");
        (u32::from_be_bytes(word) & 0x7fff_ffff) % 10_u32.pow(self.digits)
    }
}

fn hmac_bytes<M: Mac + KeyInit>(key: &[u8], message: &[u8]) -> Vec<u8> {
    let mut mac = <M as Mac>::new_from_slice(key).expect("HMAC accepts keys of any length");
    mac.update(message);
    mac.finalize().into_bytes().to_vec()
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

#[derive(Debug, PartialEq, Eq)]
struct Uri {
    name: Option<String>,
    key: Key,
}

fn strip_scheme(input: &str) -> Option<&str> {
    let (scheme, rest) = input.split_at_checked(10)?;
    scheme.eq_ignore_ascii_case("otpauth://").then_some(rest)
}

impl Uri {
    // the name is the label, with the issuer in front if the label has no ":" of its own
    fn parse(uri: &str) -> Result<Self, Error> {
        let bad = |why: &str| Error::InvalidUri(why.to_owned());

        let rest =
            strip_scheme(uri.trim()).ok_or_else(|| bad("it doesn't start with otpauth://"))?;
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

        let trimmed = |text: String| Some(text.trim().to_owned()).filter(|text| !text.is_empty());
        let name = match (trimmed(label), issuer.and_then(trimmed)) {
            (Some(label), Some(issuer)) if !label.contains(':') => {
                Some(format!("{issuer}: {label}"))
            }
            (Some(label), _) => Some(label),
            (None, issuer) => issuer,
        };

        Ok(Self { name, key })
    }
}

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

const BASE32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

// ignores case, whitespace, dashes and padding, as pasted secrets are often grouped
fn base32_decode(input: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() * 5 / 8);
    let (mut buffer, mut bits) = (0_u32, 0_u32);
    for c in input
        .chars()
        .filter(|c| !c.is_whitespace() && !matches!(c, '-' | '='))
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

fn read_json<T: DeserializeOwned + Default>(path: &Path) -> Result<T, Error> {
    match fs::read_to_string(path) {
        Ok(text) if text.trim().is_empty() => Ok(T::default()),
        Ok(text) => serde_json::from_str(&text).map_err(|source| Error::Json {
            path: path.to_owned(),
            source,
        }),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(T::default()),
        Err(source) => Err(Error::Io {
            path: path.to_owned(),
            source,
        }),
    }
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), Error> {
    let json = serde_json::to_string_pretty(value).map_err(|source| Error::Json {
        path: path.to_owned(),
        source,
    })?;
    write_private(path, json.as_bytes()).map_err(|source| Error::Io {
        path: path.to_owned(),
        source,
    })
}

// writes to a temp file first so a crash can't leave half a file; owner-only on unix
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

type Entries = IndexMap<String, String>;

// what the UI shows for one key; `expires` is unix milliseconds
#[derive(Debug, Serialize)]
struct Row<'a> {
    name: &'a str,
    code: String,
    secret: &'a str,
    valid: bool,
    expires: u64,
    period: u64,
}

// a JSON object of name -> key, in the order the keys were added
#[derive(Debug)]
pub struct Vault {
    path: PathBuf,
    entries: Entries,
}

impl Vault {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, Error> {
        let path = path.into();
        let entries = read_json(&path)?;
        Ok(Self { path, entries })
    }

    pub fn open_default() -> Result<Self, Error> {
        Self::open(Settings::load()?.vault_path()?)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn add(&mut self, name: &str, key: &str) -> Result<(), Error> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::EmptyName);
        }
        if self.entries.contains_key(name) {
            return Err(Error::Duplicate(name.to_owned()));
        }
        let stored = Key::parse(key)?.to_stored();
        self.change(|entries| {
            entries.insert(name.to_owned(), stored);
        })
    }

    // a blank `name` falls back to the one in the URI; returns the name used
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

    pub fn remove(&mut self, name: &str) -> Result<(), Error> {
        if !self.entries.contains_key(name) {
            return Err(Error::NotFound(name.to_owned()));
        }
        self.change(|entries| {
            entries.shift_remove(name);
        })
    }

    fn rows(&self, unix: u64) -> Vec<Row<'_>> {
        self.entries
            .iter()
            .map(|(name, secret)| match Key::parse(secret) {
                Ok(key) => Row {
                    name,
                    code: key.code_at(unix),
                    secret,
                    valid: true,
                    expires: (unix + key.remaining_at(unix)) * 1000,
                    period: key.period,
                },
                // an unreadable key gets a far expiry, or the UI would ask for new codes nonstop
                Err(_) => Row {
                    name,
                    code: String::new(),
                    secret,
                    valid: false,
                    expires: (unix + DEFAULT_PERIOD) * 1000,
                    period: DEFAULT_PERIOD,
                },
            })
            .collect()
    }

    // saves first, so a failed write leaves the vault as it was
    fn change(&mut self, edit: impl FnOnce(&mut Entries)) -> Result<(), Error> {
        let mut entries = self.entries.clone();
        edit(&mut entries);
        write_json(&self.path, &entries)?;
        self.entries = entries;
        Ok(())
    }
}

#[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path: Option<PathBuf>,
}

impl Settings {
    pub fn file() -> Result<PathBuf, Error> {
        dirs::config_dir()
            .map(|dir| dir.join("wauther").join("config.json"))
            .ok_or(Error::NoDataDir)
    }

    pub fn load() -> Result<Self, Error> {
        Self::load_from(&Self::file()?)
    }

    pub fn load_from(file: &Path) -> Result<Self, Error> {
        read_json(file)
    }

    pub fn vault_path(&self) -> Result<PathBuf, Error> {
        match &self.path {
            Some(path) => Ok(path.clone()),
            None => dirs::data_dir()
                .map(|dir| dir.join("wauther").join("keys.json"))
                .ok_or(Error::NoDataDir),
        }
    }

    // the old vault file is left where it was
    pub fn set_vault_path(&mut self, path: &Path, file: &Path) -> Result<PathBuf, Error> {
        let path = std::path::absolute(path).map_err(|source| Error::Io {
            path: path.to_owned(),
            source,
        })?;
        self.path = Some(path.clone());
        write_json(file, self)?;
        Ok(path)
    }
}

// what QML sees: properties rowsJson and error, slots refresh, add, addUri and remove
struct Backend {
    vault: Vault,
    rows_json: String,
    error: String,
}

#[qobject(NoQmlElement)]
impl Backend {
    qproperty!("rowsJson", Member = rows_json, Notify = rows_json_changed);
    qproperty!("error", Member = error, Notify = error_changed);

    #[qsignal]
    fn rows_json_changed(&mut self);

    #[qsignal]
    fn error_changed(&mut self);

    #[qslot]
    fn refresh(&mut self) {
        self.rows_json = rows_json(&self.vault, now());
        self.rows_json_changed();
    }

    #[qslot]
    fn add(&mut self, name: String, key: String) {
        let result = self.vault.add(&name, &key);
        self.finish(result);
    }

    #[qslot]
    fn add_uri(&mut self, uri: String, name: String) {
        let result = self.vault.add_uri(&uri, Some(&name)).map(drop);
        self.finish(result);
    }

    #[qslot]
    fn remove(&mut self, name: String) {
        let result = self.vault.remove(&name);
        self.finish(result);
    }
}

impl Backend {
    // slots can't return errors to QML, so the last one is kept in `error`
    fn finish(&mut self, result: Result<(), Error>) {
        self.error = result.err().map(|e| e.to_string()).unwrap_or_default();
        self.error_changed();
    }
}

fn rows_json(vault: &Vault, unix: u64) -> String {
    serde_json::to_string(&vault.rows(unix)).expect("rows serialize to JSON")
}

pub fn run(vault: Vault) -> i32 {
    let backend = Backend {
        vault,
        rows_json: String::new(),
        error: String::new(),
    };
    QApp::new()
        .set_initial_object("backend", Rc::new(RefCell::new(backend)))
        .load_qml(include_bytes!("Main.qml"))
        .run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SHA1_SECRET: &[u8] = b"12345678901234567890";
    const SHA256_SECRET: &[u8] = b"12345678901234567890123456789012";
    const SHA512_SECRET: &[u8] =
        b"1234567890123456789012345678901234567890123456789012345678901234";

    fn rfc_key(secret: &[u8], algorithm: Algorithm) -> Key {
        Key::new(secret.to_vec(), algorithm, 8, 30).expect("valid test key")
    }

    fn vault_in(dir: &tempfile::TempDir) -> Vault {
        Vault::open(dir.path().join("keys.json")).unwrap()
    }

    fn names(vault: &Vault) -> Vec<&str> {
        vault.entries.keys().map(String::as_str).collect()
    }

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
        assert_ne!(key.code_at(89), key.code_at(90));
    }

    #[test]
    fn a_code_keeps_its_leading_zeros() {
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

        assert_eq!(uri.key.algorithm, Algorithm::Sha512);
        assert_eq!(uri.key.digits, 8);
        assert_eq!(uri.key.period, 60);
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
        assert!(matches!(Uri::parse("é"), Err(Error::InvalidUri(_))));
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

    #[test]
    fn a_missing_vault_file_is_an_empty_vault() {
        let dir = tempfile::tempdir().unwrap();

        assert!(vault_in(&dir).entries.is_empty());
    }

    #[test]
    fn an_added_key_is_still_there_after_reopening() {
        let dir = tempfile::tempdir().unwrap();
        vault_in(&dir).add("github", "GEZDGNBV").unwrap();

        assert_eq!(names(&vault_in(&dir)), ["github"]);
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

        assert_eq!(names(&vault_in(&dir)), ["zed", "alpha", "mid"]);
    }

    #[test]
    fn adding_a_name_twice_is_an_error_and_keeps_the_first_key() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = vault_in(&dir);
        vault.add("a", "GEZDGNBV").unwrap();

        let result = vault.add("a", "MZXW6YTB");

        assert!(matches!(result, Err(Error::Duplicate(_))));
        assert_eq!(vault.entries["a"], "GEZDGNBV");
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
        assert!(vault.entries.is_empty());
        assert!(!vault.path().exists());
    }

    #[test]
    fn a_name_is_trimmed() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = vault_in(&dir);
        vault.add("  github  ", "GEZDGNBV").unwrap();

        assert_eq!(names(&vault), ["github"]);
    }

    #[test]
    fn a_removed_key_is_gone_after_reopening() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = vault_in(&dir);
        vault.add("a", "GEZDGNBV").unwrap();
        vault.add("b", "GEZDGNBV").unwrap();
        vault.remove("a").unwrap();

        assert_eq!(names(&vault_in(&dir)), ["b"]);
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
    fn a_failed_save_leaves_the_vault_as_it_was() {
        let dir = tempfile::tempdir().unwrap();
        // a directory where the file should go makes the final rename fail
        let blocked = dir.path().join("keys.json");
        fs::create_dir(&blocked).unwrap();
        let mut vault = Vault {
            path: blocked,
            entries: Entries::new(),
        };

        assert!(vault.add("a", "GEZDGNBV").is_err());
        assert!(vault.entries.is_empty());
    }

    #[test]
    fn add_uri_names_the_key_after_the_uri_label() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = vault_in(&dir);

        let name = vault
            .add_uri("otpauth://totp/Example:alice?secret=GEZDGNBV", None)
            .unwrap();

        assert_eq!(name, "Example:alice");
        assert_eq!(names(&vault), ["Example:alice"]);
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
    fn add_uri_treats_a_blank_name_as_none() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = vault_in(&dir);

        let name = vault
            .add_uri("otpauth://totp/Example:alice?secret=GEZDGNBV", Some("  "))
            .unwrap();

        assert_eq!(name, "Example:alice");
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

        let key = Key::parse(&vault.entries["a"]).unwrap();

        assert_eq!((key.digits, key.period), (8, 60));
    }

    #[test]
    fn a_hand_edited_bad_key_is_a_row_without_a_code() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("keys.json");
        fs::write(&path, r#"{"broken": "!!!"}"#).unwrap();
        let vault = Vault::open(&path).unwrap();

        let rows = vault.rows(100);

        assert!(!rows[0].valid);
        assert_eq!(rows[0].code, "");
        assert!(rows[0].expires > 100_000);
    }

    #[test]
    fn rows_carry_everything_the_ui_shows() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = vault_in(&dir);
        vault.add("a", "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ").unwrap();

        let rows = serde_json::to_value(vault.rows(59)).unwrap();

        assert_eq!(
            rows,
            json!([{
                "name": "a",
                "code": "287082",
                "secret": "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ",
                "valid": true,
                "expires": 60_000,
                "period": 30,
            }])
        );
    }

    #[test]
    fn an_empty_vault_is_an_empty_array() {
        let dir = tempfile::tempdir().unwrap();

        assert_eq!(rows_json(&vault_in(&dir), 0), "[]");
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

        assert!(Vault::open(&path).unwrap().entries.is_empty());
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
