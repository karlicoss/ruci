/*
right.. so, we need to run it against some project first
let's start with checking if it's a python project and running mypy?
it's a python prohect if:
1. .git is there, and there is a single subdir with __init__.py or we are the dir with __init__.py
*/
#[macro_use]
extern crate log;
extern crate simple_logger;
extern crate clap;
extern crate walkdir;


use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, exit};
use std::fs;
use std::thread;

use clap::{Arg, App};
use walkdir::{WalkDir, IntoIter, DirEntry};

type RuciError = String;
type RuciResult<T> = Result<T, RuciError>;

fn get_py_module_root(p: &Path) -> Result<PathBuf, RuciError> {
    fn has_init(p: &Path) -> bool {
        return p.join("__init__.py").exists();
    }
    if has_init(p) {
        return Ok(p.to_owned());
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
    if !path.is_dir() {
        return false;
    }
    if path.join(".noruci").exists() {
        return false;
        // TODO check my own git commits?
    }
    return path.join(".git").exists() || path.join(".ruci").exists();
}

fn is_ff(path: &Path, ext: &str, mimes: &[&str]) -> RuciResult<bool> {
    let ex = path.extension();
    if ex.map_or(false, |e| e == ext) { // TODO hmm. mimetype can do that..
        return Ok(true);
    }

    let meta = try!(fs::metadata(path).map_err(|_e| "error while retrieving permissions!"));
    if meta.is_dir() {
        return Ok(false);
    }

    let mode = meta.permissions().mode();
    let exec_perms = 0o100;
    // if it's not executable, it can't be imported anyway.. so not checking
    if mode | exec_perms != mode {
        return Ok(false);
    }

    let ps = try!(path.to_str().ok_or("couldn't decode the path"));
    let res = Command::new("mimetype")
        .arg("-b")
        .arg("-L")
        .arg(ps)
        .output()
        .map_err(|_e| "io error!"); // TODO include error
    let out = try!(res);
    let stdout = try!(std::str::from_utf8(&out.stdout).map_err(|_e| "io error!"));
    let mime = stdout.replace("\n", "");
    return Ok(mimes.iter().find(|&&x| x == mime).is_some());
}

fn is_py_file(path: &Path) -> Result<bool, RuciError> {
    return is_ff(path, "py", &["text/x-python3", "text/x-python"]);
}

fn is_sh_file(path: &Path) -> RuciResult<bool> {
    return is_ff(path, "sh", &["application/x-shellscript"]);
}

fn is_dotgit(entry: &DirEntry) -> bool {
    entry.file_name()
        .to_str()
        .map(|s| s == ".git")
        .unwrap_or(false)
}


fn get_sh_targets(path: &Path) -> Vec<PathBuf> {
    // kinda overkilly way to skip .git dir..
    // https://github.com/BurntSushi/walkdir
    let walker = WalkDir::new(path).into_iter();
    return walker.filter_entry(|e| !is_dotgit(e)).filter_map(
        |me| me.ok().map(|e| e.path().to_owned()).filter(|p| is_sh_file(p).unwrap_or(false))
    ).collect();
}

// TODO 1. get module(s)? then, get everything else, but don't dig into the found modules
fn get_py_targets(path: &Path) -> Vec<PathBuf> {
    // get all .py for now, later support modules..
    // TODO follow link??
    let iter = WalkDir::new(path).into_iter().filter_map(
        // TODO do not swallow errors..
        |me| me.ok().map(|e| e.path().to_owned()).filter(|p| is_py_file(p).unwrap_or(false))
    );
    return iter.collect();
}

fn check_mypy(path: &Path) -> Result<(), RuciError> {
    // if it's got a module, then check it like module? else just check all py files

    // TODO check all that conform to py code??
    // let py_module = try!(get_py_module_root(path));

    // info!("py module: {:?}", py_module);
    let targets = get_py_targets(path);

    info!("mypy: {:?}: {:?}", path, targets);
    // TODO how to handle io error?
    let res = Command::new("mypy")
        .arg("--check-untyped-defs")
        .arg("--strict-optional")
        .args(targets)
        .output()
        .expect("failed to execute process"); // TODO wtf??
    // TODO not really panic, might not be worth terminating everything

    if !res.status.success() {
        return Err(String::from_utf8(res.stdout).unwrap());
    }
    return Ok(()); // TODO meh..
}

fn check_pylint(path: &Path) -> RuciResult<()> {
    let targets = get_py_targets(path);
    info!("pylint: {:?}: {:?}", path, targets);
    if targets.is_empty() {
        return Ok(());
    }

    // TODO ugh, pylint should run separately?
    // otherwise we get
    // /L/Dropbox/repos/scripts-new/wm/other-monitor: error: Duplicate module named '__main__'
    let res = try!(Command::new("pylint")  // TODO maybe, python3 -m pylint?
        .arg("-E")
        .args(targets)
        .output()
        .map_err(|e| format!("error while executing pylint {:?}", e)));
    if !res.status.success() {
        return Err(String::from_utf8(res.stdout).unwrap());
    }
    return Ok(());
}

fn check_shellcheck(path: &Path) -> RuciResult<()> {
    let targets = get_sh_targets(path);
    // TODO hmm, need to exlude .git directory?
    info!("shellcheck: {:?}: {:?}", path, targets);
    if targets.is_empty() {
        return Ok(());
    }

    let res = try!(Command::new("shellcheck")
                   .args(targets)
                   .output()
                   .map_err(|e| format!("error while executing shellecheck {:?}", e)));
    if !res.status.success() {
        return Err(String::from_utf8(res.stdout).unwrap());
    }
    return Ok(())
}

fn check_dir(path: &Path) -> RuciResult<()> {
    let pc = path.to_owned();
    let mypy = thread::spawn(move || {
        return check_mypy(&pc);
    });

    // TODO really, this is how it's done?..
    let pc2 = path.to_owned();
    let pylint = thread::spawn(move || {
        return check_pylint(&pc2);
    });

    let sc = path.to_owned();
    let shellcheck = thread::spawn(move || {
        return check_shellcheck(&sc);
    });


    let checks = vec![
        mypy,
        pylint,
        shellcheck,
    ];

    let mut chresults = vec![];
    for c in checks {
        let res = c.join();
        chresults.push(res.unwrap()); // TODO handle errors?
    }

    // TODO err, why into_iter works for vector but not for array?
    // ah! iter() is always borrowing?
    let out: Vec<String> = chresults.into_iter().filter_map(|thing: RuciResult<()>| {
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

    let matches = App::new("RuCi")
                        .about("Quickchecks stuff")
                        .arg(Arg::with_name("path")
                            // .long("path")
                            // .short("p")
                            .multiple(true)
                            // .index(1)
                            // .value_name("path")
                             // .takes_value(true)
                        )
                        .get_matches();

    // Gets a value for config if supplied by user, or defaults to "default.conf"
    let paths = matches.values_of("path"); // TODO or current dir??_or(["."]);
    // println!("Value for config: {}", config);

    let targets: Vec<_> = paths.map(|ps| ps.collect()).unwrap_or(vec!["."]);

    /*
       eh, ok, this looks a bit meh.
       on the one hand, we wanna output stuff ASAP during processing, just a nice thing to do
       on the other hand, logging is not badly mutable and it's nice to implement this without explcit for loop
    */
    let errors: Vec<_> = targets.iter()
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
                Err(e) => error!("... error\n{}", e),
            }
            return Option::from(res);
    }).filter(Result::is_err).collect();

    exit(if errors.is_empty() {0} else {1});
}
// TODO cache .. maybe run mypy while updating cache?

// TODO maybe it could also generate json report? Then we could track when something broke.. and notify depending on importance (e.g. kython or kron are pretty important)
