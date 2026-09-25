use crate::exit;
use crate::{config, util};
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};

const DEFAULT_VERSION: &str = "stable";

pub struct Spec {
    prefix: PathBuf,
    input: PathBuf,
    case_name: String,
    case_dir: PathBuf,
    program_args: Vec<OsString>,
}

impl Spec {
    pub fn new(input: PathBuf, version: Option<String>) -> Result<Self, Box<dyn Error>> {
        let version = version.unwrap_or(DEFAULT_VERSION.to_string());
        let (case_dir, case_name) = split_input_into_dir_and_case(&input);
        let prefix = fs::canonicalize(config::karsksal_root().join("versions").join(&version))
            .map_err(|e| {
                format!(
                    "Couldn't find version {:?} in {:?}: {e}",
                    version,
                    config::karsksal_root()
                )
            })?;

        Ok(Self {
            prefix,
            input,
            case_dir,
            case_name,
            program_args: Vec::new(),
        })
    }

    pub fn get_input(&self) -> &Path {
        self.input.as_path()
    }

    /// Get the prefix for the given version's environment
    pub fn get_prefix(&self) -> &Path {
        self.prefix.as_path()
    }

    /// The name of the input file without the file extension, or the name of the directory
    pub fn get_case_name(&self) -> &str {
        &self.case_name
    }

    /// The path to the directory containing the input file
    pub fn get_case_dir(&self) -> &Path {
        &self.case_dir
    }

    pub fn get_bin<S: AsRef<Path>>(&self, name: S) -> PathBuf {
        self.prefix.join("bin").join(name)
    }

    pub fn arg<S: AsRef<OsStr>>(&mut self, arg: S) -> &mut Self {
        self.program_args.push(arg.as_ref().to_os_string());
        self
    }

    pub fn args<S: AsRef<OsStr>>(&mut self, args: &[S]) -> &mut Self {
        for arg in args {
            self.program_args.push(arg.as_ref().to_os_string());
        }
        self
    }

    pub fn get_args(&self) -> &Vec<OsString> {
        &self.program_args
    }
}

fn split_input_into_dir_and_case(input_file: &Path) -> (PathBuf, String) {
    let name = |s: Option<&OsStr>| {
        s.and_then(OsStr::to_str)
            .expect("Couldn't get file name from path")
            .to_string()
    };

    if input_file.is_dir() {
        (input_file.to_path_buf(), name(input_file.file_name()))
    } else {
        (
            input_file.parent().unwrap().to_owned(),
            name(input_file.file_stem()),
        )
    }
}

pub fn get_install_root(version: String) -> PathBuf {
    let versions_dir = config::karsksal_root().join("versions");
    let path = versions_dir.join(&version);
    fs::canonicalize(path).unwrap_or_else(|err| {
        exit!("Error finding version '{}'\nLooked in '{}'\nUse `{} --print-versions` to see a list of valid versions\n{}", version, versions_dir.display(), util::BIN_NAME.get(), err);
    })
}
