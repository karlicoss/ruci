mod config;

/*
right.. so, we need to run it against some project first
let's start with checking if it's a python project and running mypy?
it's a python prohect if:
1. .git is there, and there is a single subdir with __init__.py or we are the dir with __init__.py
*/
#[macro_use]
extern crate log;
extern crate simple_logger;

use std::path::{Path, PathBuf};
use std::process::{Command, exit};
use std::fs;

type RuciError = String;
type RuciResult = Result<(), RuciError>;

fn get_py_module_root(p: &Path) -> Result<PathBuf, RuciError> {
    fn has_init(p: &Path) -> bool {
        return p.join("__init__.py").exists();
    }
    if has_init(p) {
        return Ok(PathBuf::from(p));
    }
    let filtered: Vec<_> = fs::read_dir(p).unwrap().filter_map(|d| {
        // TODO why as_ref here???
        let pp = d.as_ref().unwrap().path();
        if pp.is_dir() && has_init(&pp) {
            Option::from(pp)
        } else {
            Option::None
        }
    }).collect();
    if filtered.len() != 1 {
       return Err(format!("{:?}", filtered));
    } else {
        // TODO ok, it's a bit better than return Ok(filtered.remove(0)), but still ugly
        // and I guess that better than return Ok(filtered.into_iter().next().unwrap()) as well..
        return Ok(filtered.get(0).unwrap().clone());
    }
}

fn is_interesting(path: &Path) -> bool {
    return path.is_dir() && path.join(".git").exists();
}

fn check_mypy(path: &Path) -> Result<(), RuciError> {
    let py_module = try!(get_py_module_root(path));
    // TODO how to handle io error?
    let res = Command::new("mypy")
        .arg(py_module)
        .output()
        .expect("failed to execute process");
    // TODO not really panic, might not be worth terminating everything

    if !res.status.success() {
        return Err(String::from_utf8(res.stdout).unwrap());
    }
    return Ok(()); // TODO meh..
}

fn check_shellcheck(_path: &Path) -> Result<(), RuciError> {
    return Err(String::from("TODO IMPLEMENT SHELLCHECK"));
}

fn check_dir(path: &Path) -> RuciResult {
    let checks = [
        check_mypy(path),
        check_shellcheck(path),
    ].to_vec();
    // TODO err, why into_iter works for vector but not for array?
    // ah! iter() is always borrowing?
    let out: Vec<String> = checks.into_iter().filter_map(|thing: RuciResult| {
        match thing {
            Ok(_)  => Option::None,
            Err(e) => Option::from(e),
        }
    }).collect();
    if out.len() == 0 {
        return Ok(())
    } else {
        return Err(out.join("\n"))
    }
}

extern crate walkdir;
use walkdir::{WalkDir, IntoIter};


struct Interesting {
    iter: IntoIter,
}

impl Interesting {
    fn walk(p: &Path) -> Self {
        Self {
            iter: WalkDir::new(p).into_iter()
        }
    }
}

impl Iterator for Interesting {
    type Item = PathBuf;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let entry = match self.iter.next() {
                None => break,
                Some(Err(err)) => panic!("ERROR: {}", err),
                Some(Ok(entry)) => entry,
            };
            if !entry.file_type().is_dir() {
                continue
            }
            let ep = entry.path();
            if is_interesting(ep) {
                self.iter.skip_current_dir();
                return Some(PathBuf::from(ep))
            }
        }
        None
    }
}


fn main() {
    simple_logger::init().unwrap();

    /*
       eh, ok, this looks a bit meh.
       on the one hand, we wanna output stuff ASAP during processing, just a nice thing to do
       on the other hand, logging is not badly mutable and it's nice to implement this without explcit for loop
    */
    let errors: Vec<_> = config::TARGETS.iter()
        .flat_map(|ps| Interesting::walk(Path::new(ps)))
        .filter_map(|target| {
            info!("checking {:?}", target);
            if !is_interesting(&target) {
                warn!("target is not interesting... skipping!");
                return Option::None;
            }
            let res = check_dir(&target);
            match &res {
                Ok(_) => info!("... succcess!"),
                Err(e) => error!("... error {}", e),
            }
            return Option::from(res);
    }).filter(Result::is_err).collect();

    exit(if errors.is_empty() {0} else {1});
}
