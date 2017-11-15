/*
 * Copyright 2017 Trevor Bentley
 *
 * Author: Trevor Bentley
 * Contact: trevor@trevorbentley.com
 * Source: https://github.com/mrmekon/circadian
 *
 * This file is part of Circadian.
 *
 * Circadian is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * Circadian is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with Circadian.  If not, see <http://www.gnu.org/licenses/>.
 */
extern crate regex;
use regex::Regex;

extern crate glob;
use glob::glob;

extern crate clap;

extern crate ini;
use ini::Ini;

extern crate nix;
use nix::sys::signal;

extern crate time;

use std::error::Error;
use std::process::Stdio;
use std::process::Command;
use std::sync::atomic::{AtomicBool, ATOMIC_BOOL_INIT, Ordering};

use std::os::unix::process::CommandExt;

pub struct CircadianError(String);

impl std::fmt::Display for CircadianError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::fmt::Debug for CircadianError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for CircadianError {
    fn description(&self) -> &str {
        self.0.as_str()
    }

    fn cause(&self) -> Option<&std::error::Error> {
        None
    }
}
impl <'a> From<&'a str> for CircadianError {
    fn from(error: &str) -> Self {
        CircadianError(error.to_owned())
    }
}
impl From<std::io::Error> for CircadianError {
    fn from(error: std::io::Error) -> Self {
        CircadianError(error.description().to_owned())
    }
}
impl From<regex::Error> for CircadianError {
    fn from(error: regex::Error) -> Self {
        CircadianError(error.description().to_owned())
    }
}
impl From<std::num::ParseIntError> for CircadianError {
    fn from(error: std::num::ParseIntError) -> Self {
        CircadianError(error.description().to_owned())
    }
}
impl From<std::string::FromUtf8Error> for CircadianError {
    fn from(error: std::string::FromUtf8Error) -> Self {
        CircadianError(error.description().to_owned())
    }
}
impl From<glob::PatternError> for CircadianError {
    fn from(error: glob::PatternError) -> Self {
        CircadianError(error.description().to_owned())
    }
}
impl From<ini::ini::Error> for CircadianError {
    fn from(error: ini::ini::Error) -> Self {
        CircadianError(error.description().to_owned())
    }
}
impl From<nix::Error> for CircadianError {
    fn from(error: nix::Error) -> Self {
        CircadianError(error.description().to_owned())
    }
}

type IdleResult = Result<u32, CircadianError>;
type ThreshResult = Result<bool, CircadianError>;
type ExistResult = Result<bool, CircadianError>;

#[allow(dead_code)]
enum NetConnection {
    SSH,
    SMB
}

#[allow(dead_code)]
enum CpuHistory {
    Min1,
    Min5,
    Min15
}

#[derive(Debug)]
struct IdleResponse {
    w_idle: IdleResult,
    w_enabled: bool,
    xssstate_idle: IdleResult,
    xssstate_enabled: bool,
    xprintidle_idle: IdleResult,
    xprintidle_enabled: bool,
    tty_idle: u32,
    tty_enabled: bool,
    x11_idle: u32,
    x11_enabled: bool,
    min_idle: u32,
    idle_target: u64,
    idle_remain: u64,
    is_idle: bool,
}
impl std::fmt::Display for IdleResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let result_map = vec![
            (self.w_idle.as_ref(), self.w_enabled, "w"),
            (self.xssstate_idle.as_ref(), self.xssstate_enabled, "xssstate"),
            (self.xprintidle_idle.as_ref(), self.xprintidle_enabled, "xprintidle"),
        ];
        for (var,enabled,name) in result_map {
            let s = match var {
                Ok(x)  => x.to_string(),
                Err(e) => e.to_string(),
            };
            let enabled = match enabled {
                true => "*",
                _ => "",
            };
            let name = format!("{}{}", name, enabled);
            let _ = write!(f, "{:<16}: {}\n", name, s);
        }
        let int_map = vec![
            (self.tty_idle, self.tty_enabled, "TTY (combined)"),
            (self.x11_idle, self.x11_enabled, "X11 (combined)"),
        ];
        for (var,enabled,name) in int_map {
            let enabled = match enabled {
                true => "*",
                _ => "",
            };
            let name = format!("{}{}", name, enabled);
            let _ = write!(f, "{:<16}: {}\n", name, var);
        }
        let _ = write!(f, "{:<16}: {}\n", "Idle (min)", self.min_idle);
        let _ = write!(f, "{:<16}: {}\n", "Idle target", self.idle_target);
        let _ = write!(f, "{:<16}: {}\n", "Until idle", self.idle_remain);
        let _ = write!(f, "{:<16}: {}\n", "IDLE?", self.is_idle);
        Ok(())
    }
}

#[derive(Debug)]
struct NonIdleResponse {
    cpu_load: ThreshResult,
    cpu_load_enabled: bool,
    ssh: ExistResult,
    ssh_enabled: bool,
    smb: ExistResult,
    smb_enabled: bool,
    audio: ExistResult,
    audio_enabled: bool,
    procs: ExistResult,
    procs_enabled: bool,
    is_blocked: bool,
}
impl std::fmt::Display for NonIdleResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let result_map = vec![
            (self.cpu_load.as_ref(), self.cpu_load_enabled, "CPU load"),
            (self.ssh.as_ref(), self.ssh_enabled, "SSH"),
            (self.smb.as_ref(), self.smb_enabled, "SMB"),
            (self.audio.as_ref(), self.audio_enabled, "Audio"),
            (self.procs.as_ref(), self.procs_enabled, "Processes"),
        ];
        for (var,enabled,name) in result_map {
            let s = match var {
                Ok(x)  => x.to_string(),
                Err(e) => e.to_string(),
            };
            let enabled = match enabled {
                true => "*",
                _ => "",
            };
            let name = format!("{}{}", name, enabled);
            let _ = write!(f, "{:<16}: {}\n", name, s);
        }
        let _ = write!(f, "{:<16}: {}\n", "BLOCKED?", self.is_blocked);
        Ok(())
    }
}

static SIGUSR_SIGNALED: AtomicBool = ATOMIC_BOOL_INIT;

/// Set global flag when SIGUSR1 signal is received
extern fn sigusr1_handler(_: i32) {
    SIGUSR_SIGNALED.store(true, Ordering::Relaxed);
}

/// Register SIGUSR1 signal handler
fn register_sigusr1() -> Result<signal::SigAction, CircadianError> {
    let sig_handler = signal::SigHandler::Handler(sigusr1_handler);
    let sig_action = signal::SigAction::new(sig_handler,
                                            signal::SaFlags::empty(),
                                            signal::SigSet::empty());
    unsafe {
        Ok(signal::sigaction(signal::SIGUSR1, &sig_action)?)
    }
}

/// Parse idle time strings from 'w' command into seconds
fn parse_w_time(time_str: &str) -> Result<u32, CircadianError> {
    let mut secs: u32 = 0;
    let mut mins: u32 = 0;
    let mut hours:u32 = 0;
    let re_sec = Regex::new(r"^\d+.\d+s$")?;
    let re_min = Regex::new(r"^\d+:\d+$")?;
    let re_hour = Regex::new(r"^\d+:\d+m$")?;
    if re_sec.is_match(time_str) {
        let time_str: &str = time_str.trim_matches('s');
        let parts: Vec<u32> = time_str.split(".")
            .map(|s| str::parse::<u32>(s).unwrap_or(0))
            .collect();
        secs = *parts.get(0).unwrap_or(&0);
    }
    else if re_min.is_match(time_str) {
        let parts: Vec<u32> = time_str.split(":")
            .map(|s| str::parse::<u32>(s).unwrap_or(0))
            .collect();
        mins = *parts.get(0).unwrap_or(&0);
        secs = *parts.get(1).unwrap_or(&0);
    }
    else if re_hour.is_match(time_str) {
        let time_str: &str = time_str.trim_matches('m');
        let parts: Vec<u32> = time_str.split(":")
            .map(|s| str::parse::<u32>(s).unwrap_or(0))
            .collect();
        hours = *parts.get(0).unwrap_or(&0);
        mins = *parts.get(1).unwrap_or(&0);
    }
    else {
        return Err(CircadianError("Invalid idle format".to_string()));
    }
    Ok((hours*60*60) + (mins*60) + secs)
}

/// Call 'w' command and return minimum idle time
fn idle_w() -> IdleResult {
    let w_stdout = Stdio::piped();
    let mut w_output = Command::new("w")
        .arg("-hus")
        .stdout(w_stdout).spawn()?;
    let _ = w_output.wait()?;
    let w_stdout = w_output.stdout
        .ok_or(CircadianError("w command has no output".into()))?;
    let awk_output = Command::new("awk")
        .arg("{print $4}")
        .stdin(w_stdout)
        .output()?;
    let idle_times: Vec<u32> = String::from_utf8(awk_output.stdout)
        .unwrap_or(String::new())
        .split("\n")
        .filter(|t| t.len() > 0)
        .map(|t| parse_w_time(t))
        .filter_map(|t| t.ok())
        .collect();
    Ok(idle_times.iter().cloned().fold(std::u32::MAX, std::cmp::min))
}

/// Call idle command for each X display
///
/// 'cmd' should be a unix command that, given args 'args', prints the idle
/// time in milliseconds.  It will be run with the DISPLAY env variable set
/// and with the uid of the user that owns the DISPLAY, for every running
/// X display.  The minimum of all found idle times is returned.
fn idle_fn(cmd: &str, args: Vec<&str>) -> IdleResult {
    let mut display_mins: Vec<u32> = Vec::<u32>::new();
    for device in glob("/tmp/.X11-unix/X*")? {
        let device: String = match device {
            Ok(p) => p.to_str().unwrap_or("0").to_owned(),
            _ => "0".to_owned(),
        };
        let display = format!(":{}", device.chars().rev().next().unwrap_or('0'));
        let mut output = Command::new("w")
            .arg("-hus")
            .stdout(Stdio::piped()).spawn()?;
        let _ = output.wait()?;
        let w_stdout = output.stdout
            .ok_or(CircadianError("w command has no output".into()))?;
        let awk_arg = format!("{{if ($3 ~ /^{}/) print $1}}", display);
        let output = Command::new("awk")
            .arg(awk_arg)
            .stdin(w_stdout)
            .output()?;
        let user_str = String::from_utf8(output.stdout)
            .unwrap_or(String::new());
        let user = user_str.split("\n").next().unwrap_or("root");
        let output = Command::new("id")
            .arg("-u")
            .arg(user)
            .output()?;
        let mut uid = String::from_utf8(output.stdout)
            .unwrap_or(String::new());
        uid.pop();
        let uid = uid.parse::<u32>().unwrap_or(0);
        let output = Command::new(cmd)
            .args(&args)
            .uid(uid)
            .env("DISPLAY", display)
            .output()?;
        let mut idle_str = String::from_utf8(output.stdout)
            .unwrap_or(String::new());
        idle_str.pop();
        display_mins.push(idle_str.parse::<u32>().unwrap_or(std::u32::MAX)/1000)
    }
    match display_mins.len() {
        0 => Err(CircadianError("No displays found.".to_string())),
        _ => Ok(display_mins.iter().fold(std::u32::MAX, |acc, x| std::cmp::min(acc,*x)))
    }
}

/// Call 'xprintidle' command and return idle time
fn idle_xprintidle() -> IdleResult {
    idle_fn("xprintidle", vec![])
}

/// Call 'xssstate' command and return idle time
fn idle_xssstate() -> IdleResult {
    idle_fn("xssstate", vec!["-i"])
}


/// Compare whether 'uptime' 5-min CPU usage compares
/// to the given thresh with the given cmp function.
///
/// ex: thresh_cpu(CpuHistory::Min1, 0.1, std::cmp::PartialOrd::lt) returns true
///     if the 5-min CPU usage is less than 0.1 for the past minute
///
fn thresh_cpu<C>(history: CpuHistory, thresh: f64, cmp: C) -> ThreshResult
    where C: Fn(&f64, &f64) -> bool {
    let output = Command::new("uptime")
        .output()?;
    let uptime_str = String::from_utf8(output.stdout)
        .unwrap_or(String::new());
    let columns: Vec<&str> = uptime_str.split(" ").collect();
    let cpu_usages: Vec<f64> = columns.iter()
        .rev().take(3).map(|x| *x).collect::<Vec<&str>>().iter()
        .rev()
        .map(|x| *x)
        .filter(|x| x.len() > 0)
        .map(|x| str::parse::<f64>(&x[0..x.len()-1].replace(",",".")).unwrap_or(0.0))
        .collect::<Vec<f64>>();
    let idle: Vec<bool> = cpu_usages.iter()
        .map(|x| cmp(x, &thresh))
        .collect();
    // idle is bools of [1min, 5min, 15min] CPU usage
    let idx = match history {
        CpuHistory::Min1 => 0,
        CpuHistory::Min5 => 1,
        CpuHistory::Min15 => 2,
    };
    // false == below threshold, true == above
    Ok(!*idle.get(idx).unwrap_or(&false))
}

/// Determine whether a process (by name regex) is running.
fn exist_process(prc: &str) -> ExistResult {
    let output = Command::new("pgrep")
        .arg("-c")
        .arg(prc)
        .output()?;
    let output = &output.stdout[0..output.stdout.len()-1];
    let count: u32 = String::from_utf8(output.to_vec())
        .unwrap_or(String::new()).parse::<u32>()?;
    Ok(count > 0)
}

/// Determine whether the given type of network connection is established.
fn exist_net_connection(conn: NetConnection) -> ExistResult {
    let mut output = Command::new("netstat")
        .arg("-tnpa")
        .stderr(Stdio::null())
        .stdout(Stdio::piped()).spawn()?;
    let _ = output.wait()?;
    let stdout = output.stdout
        .ok_or(CircadianError("netstat command has no output".to_string()))?;
    let mut output = Command::new("grep")
        .arg("ESTABLISHED")
        .stdin(stdout)
        .stdout(Stdio::piped()).spawn()?;
    let _ = output.wait()?;
    let stdout = output.stdout
        .ok_or(CircadianError("netstat command has no connections".to_string()))?;
    let pattern = match conn {
        NetConnection::SSH => "[0-9]+/ssh[d]*",
        NetConnection::SMB => "[0-9]+/smb[d]*",
    };
    let output = Command::new("grep")
        .arg("-E")
        .arg(pattern)
        .stdin(stdout)
        .output()?;
    let output = String::from_utf8(output.stdout)
        .unwrap_or(String::new());
    let connections: Vec<&str> = output
        .split("\n")
        .filter(|l| l.len() > 0)
        .collect();
    Ok(connections.len() > 0)
}

/// Determine whether audio is actively playing on any ALSA interface.
fn exist_audio() -> ExistResult {
    let mut count = 0;
    for device in glob("/proc/asound/card*/pcm*/sub*/status")? {
        if let Ok(path) = device {
            let mut cat_output = Command::new("cat")
                .arg(path)
                .stderr(Stdio::null())
                .stdout(Stdio::piped()).spawn()?;
            let _ = cat_output.wait()?;
            let stdout = cat_output.stdout
                .ok_or(CircadianError("pacmd failed".to_string()))?;
            let output = Command::new("grep")
                .arg("state:")
                .stdin(stdout)
                .output()?;
            let output_str = String::from_utf8(output.stdout)?;
            let lines: Vec<&str> = output_str.split("\n")
                .filter(|l| l.len() > 0)
                .collect();
            count += lines.len();
        }
    }
    Ok(count > 0)
}

struct CircadianLaunchOptions {
    config_file: String,
    //script_dir: String,
}

#[derive(Default,Debug)]
struct CircadianConfig {
    idle_time: u64,
    auto_wake: Option<String>,
    on_idle: Option<String>,
    on_wake: Option<String>,
    tty_input: bool,
    x11_input: bool,
    ssh_block: bool,
    smb_block: bool,
    audio_block: bool,
    max_cpu_load: Option<f64>,
    process_block: Vec<String>,
}

fn read_config(file_path: &str) -> Result<CircadianConfig, CircadianError> {
    println!("Reading config from file: {}", file_path);
    let i = Ini::load_from_file(file_path)?;
    let mut config: CircadianConfig = Default::default();
    if let Some(section) = i.section(Some("actions".to_owned())) {
        config.idle_time = section.get("idle_time")
            .map_or(0, |x| if x.len() > 0 {
                let (body,suffix) = x.split_at(x.len()-1);
                let num: u64 = match suffix {
                    "m" => body.parse::<u64>().unwrap_or(0) * 60,
                    "h" => body.parse::<u64>().unwrap_or(0) * 60 * 60,
                    _ => x.parse::<u64>().unwrap_or(0),
                };
                num
            } else {0});
        config.auto_wake = section.get("auto_wake")
            .and_then(|x| if x.len() > 0 {Some(x.to_owned())} else {None});
        config.on_idle = section.get("on_idle")
            .and_then(|x| if x.len() > 0 {Some(x.to_owned())} else {None});
        config.on_wake = section.get("on_wake")
            .and_then(|x| if x.len() > 0 {Some(x.to_owned())} else {None});
    }
    fn read_bool(s: &std::collections::HashMap<String,String>,
                 key: &str) -> bool {
        match s.get(key).unwrap_or(&"no".to_string()).to_lowercase().as_str() {
            "yes" | "true" | "1" => true,
            _ => false,
        }
    }
    if let Some(section) = i.section(Some("heuristics".to_owned())) {
        config.tty_input = read_bool(section, "tty_input");
        config.x11_input = read_bool(section, "x11_input");
        config.ssh_block = read_bool(section, "ssh_block");
        config.smb_block = read_bool(section, "smb_block");
        config.audio_block = read_bool(section, "audio_block");
        config.max_cpu_load = section.get("max_cpu_load")
            .and_then(|x| if x.len() > 0
                      {Some(x.parse::<f64>().unwrap_or(999.0))} else {None});
        if let Some(proc_str) = section.get("process_block") {
            let proc_list = proc_str.split(",");
            config.process_block = proc_list
                .map(|x| x.trim().to_owned()).collect();
        }
    }
    Ok(config)
}

fn read_cmdline() -> CircadianLaunchOptions {
    let matches = clap::App::new("circadian")
        .version(env!("CARGO_PKG_VERSION"))
        .author(env!("CARGO_PKG_AUTHORS"))
        .about(env!("CARGO_PKG_DESCRIPTION"))
        .args_from_usage(
            "-f, --config=[FILE] ''
             -d, --script-dir=[DIR] ''")
        .get_matches();
    let config = matches.value_of("config").unwrap_or("/etc/circadian.conf");
    //let script_dir = matches.value_of("script-dir").unwrap_or("");
    //println!("Script dir: {}", script_dir);
    CircadianLaunchOptions {
        config_file: config.to_owned(),
        //script_dir: script_dir.to_owned(),
    }
}

fn test_idle(config: &CircadianConfig) -> IdleResponse {
    let tty = idle_w();
    let xssstate = idle_xssstate();
    let xprintidle = idle_xprintidle();
    let tty_idle = *tty.as_ref().unwrap_or(&std::u32::MAX);
    let x11_idle = std::cmp::min(*xssstate.as_ref().unwrap_or(&std::u32::MAX),
                                 *xprintidle.as_ref().unwrap_or(&std::u32::MAX));
    let min_idle: u32 = match (config.tty_input, config.x11_input) {
        (true,true) => std::cmp::min(tty_idle, x11_idle) as u32,
        (true,false) => tty_idle as u32,
        (false,_) => x11_idle as u32,
    };
    let idle_remain: u64 =
        std::cmp::max(config.idle_time as i64 - min_idle as i64, 0) as u64;
    IdleResponse {
        w_idle: tty,
        w_enabled: config.tty_input,
        xssstate_idle: xssstate,
        xssstate_enabled: config.x11_input,
        xprintidle_idle: xprintidle,
        xprintidle_enabled: config.x11_input,
        tty_idle: tty_idle,
        tty_enabled: config.tty_input,
        x11_idle: x11_idle,
        x11_enabled: config.x11_input,
        min_idle: min_idle,
        idle_target: config.idle_time,
        idle_remain: idle_remain,
        is_idle: idle_remain == 0,
    }
}
fn test_nonidle(config: &CircadianConfig) -> NonIdleResponse {
    let cpu_load = thresh_cpu(CpuHistory::Min1,
                              config.max_cpu_load.unwrap_or(999.0),
                              std::cmp::PartialOrd::lt);
    let cpu_load_enabled = config.max_cpu_load.is_some() &&
        config.max_cpu_load.unwrap() < 999.0;
    let ssh = exist_net_connection(NetConnection::SSH);
    let ssh_enabled = config.ssh_block;
    let smb = exist_net_connection(NetConnection::SMB);
    let smb_enabled = config.smb_block;
    let audio = exist_audio();
    let audio_enabled = config.audio_block;
    let procs = config.process_block.iter()
    // Run 'exist_process' on each process string
        .map(|p| exist_process(p))
    // Flatten into a single result with a Vec of bools
        .collect::<Result<Vec<bool>, CircadianError>>()
    // Flatten Vec of bools into a single bool
        .map(|x| x.iter().fold(false, |acc,p| acc || *p));
    let procs_enabled = config.process_block.len() > 0;

    let blocked = (cpu_load_enabled && *cpu_load.as_ref().unwrap_or(&true)) ||
        (ssh_enabled && *ssh.as_ref().unwrap_or(&true)) ||
        (smb_enabled && *smb.as_ref().unwrap_or(&true)) ||
        (audio_enabled && *audio.as_ref().unwrap_or(&true)) ||
        (procs_enabled && *procs.as_ref().unwrap_or(&true));
    NonIdleResponse {
        cpu_load: cpu_load,
        cpu_load_enabled: cpu_load_enabled,
        ssh: ssh,
        ssh_enabled: ssh_enabled,
        smb: smb,
        smb_enabled: smb_enabled,
        audio: audio,
        audio_enabled: audio_enabled,
        procs: procs,
        procs_enabled: procs_enabled,
        is_blocked: blocked,
    }
}

#[allow(dead_code)]
fn test() {
    println!("Sec: {:?}", parse_w_time("10.45s"));
    println!("Sec: {:?}", parse_w_time("1:11"));
    println!("Sec: {:?}", parse_w_time("0:10m"));
    println!("w min: {:?}", idle_w());
    println!("xssstate min: {:?}", idle_xssstate());
    println!("xprintidle min: {:?}", idle_xprintidle());
    println!("cpu: {:?}", thresh_cpu(CpuHistory::Min5, 0.3, std::cmp::PartialOrd::lt));
    println!("ssh: {:?}", exist_net_connection(NetConnection::SSH));
    println!("smb: {:?}", exist_net_connection(NetConnection::SMB));
    println!("iotop: {:?}", exist_process("^iotop$"));
    println!("audio: {:?}", exist_audio());
}

fn main() {
    println!("Circadian launching.");
    let launch_opts = read_cmdline();
    let config = read_config(&launch_opts.config_file)
        .unwrap_or_else(|x| {
            println!("{}", x);
            println!("Could not open config file.  Exiting.");
            std::process::exit(1);
        });
    println!("{:?}", config);
    if !config.tty_input && !config.x11_input {
        println!("tty_input or x11_input must be enabled.  Exiting.");
        std::process::exit(1);
    }
    if config.tty_input && idle_w().is_err() {
        println!("'w' command required by tty_input failed.  Exiting.");
        std::process::exit(1);
    }
    if config.x11_input &&
        idle_xssstate().is_err() &&
        idle_xprintidle().is_err() {
            println!("Both 'xssstate' and 'xprintidle' commands required by x11_input failed.  Exiting.");
            std::process::exit(1);
        }
    if config.max_cpu_load.is_some() &&
        thresh_cpu(CpuHistory::Min1, 0.0, std::cmp::PartialOrd::lt).is_err() {
            println!("'uptime' command required by max_cpu_load failed.  Exiting.");
            std::process::exit(1);
        }
    if (config.ssh_block || config.smb_block) &&
        exist_net_connection(NetConnection::SSH).is_err() {
        println!("'netstat' command required by ssh/smb_block failed.  Exiting.");
        std::process::exit(1);
        }
    if config.audio_block && exist_audio().is_err() {
        println!("'/proc/asound/' required by audio_block is unreadable.  Exiting.");
        std::process::exit(1);
    }
    if config.process_block.len() > 0 && exist_process("").is_err() {
        println!("'pgrep' required by process_block failed.  Exiting.");
        std::process::exit(1);
    }
    if config.idle_time == 0 {
        println!("Idle time disabled.  Nothing to do.  Exiting.");
        std::process::exit(1);
    }

    let _ = register_sigusr1().unwrap_or_else(|x| {
        println!("{}", x);
        println!("WARNING: Could not register SIGUSR1 handler.");
        std::process::exit(1);
    });
    println!("Configuration valid.  Idle detection starting.");

    let mut start = time::now_utc().to_timespec().sec as i64;
    loop {
        let idle = test_idle(&config);
        if idle.is_idle {
            let tests = test_nonidle(&config);
            if !tests.is_blocked {
                println!("Idle Detection Summary:\n{}{}", idle, tests);
                println!("IDLE DETECTED.");
                // TODO: on_idle()
                //let status = Command::new("systemctl")
                //    .arg("suspend")
                //    .status().unwrap();
                //println!("Suspend status: {}", status);
            }
        }

        let sleep_time = std::cmp::max(idle.idle_remain, 5000);
        let sleep_chunk = 500;
        // Sleep for the minimum time needed before the system can possibly
        // be idle, but do it in small chunks so we can periodically check
        // for signals and clock jumps.
        for _ in 0 .. sleep_time / sleep_chunk {
            // Print stats when SIGUSR1 signal received
            let signaled = SIGUSR_SIGNALED.swap(false, Ordering::Relaxed);
            if signaled {
                let idle = test_idle(&config);
                let tests = test_nonidle(&config);
                println!("Idle Detection Summary:\n{}{}", idle, tests);
            }

            let now = time::now_utc().to_timespec().sec as i64;
            // Look for clock jumps, that indicate the system slept
            if start + 120 < now {
                println!("Watchdog missed.  Wake from sleep!");
                let idle = test_idle(&config);
                let tests = test_nonidle(&config);
                println!("Idle Detection Summary:\n{}{}", idle, tests);
            }
            // Kick watchdog timer once per minute
            if start + 60 < now {
                start = time::now_utc().to_timespec().sec as i64;
            }
            std::thread::sleep(std::time::Duration::from_millis(sleep_chunk));
        }
    }
}
