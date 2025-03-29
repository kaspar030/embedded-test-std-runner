use std::{
    io::{BufRead, Read},
    process::{Command, ExitCode, Stdio},
    time::Duration,
};

use anyhow::Result;
use libtest_mimic::{Arguments, Failed, Trial};
use serde::{Deserialize, Serialize};

const DEFAULT_TIMEOUT: u32 = 60;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let test_bin = args.next().unwrap();
    let mut args = Arguments::from_iter(args);
    args.test_threads = Some(1);

    let tests = list_tests(&test_bin).unwrap();

    libtest_mimic::run(&args, tests).exit_code()
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Tests {
    pub version: u32,
    pub tests: Vec<Test>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Test {
    pub name: String,
    pub should_panic: bool,
    pub ignored: bool,
    pub timeout: Option<u32>,
}

fn list_tests(test_bin: &str) -> Result<Vec<Trial>> {
    fn convert_test(test: Test, test_bin: &str) -> Trial {
        use wait_timeout::ChildExt;
        let test_bin = test_bin.to_string();
        let test_name = test.name.clone();
        let test_timeout = test.timeout.unwrap_or(DEFAULT_TIMEOUT);

        Trial::test(test.name.clone(), move || {
            let mut child = Command::new(test_bin)
                .arg("run")
                .arg(test_name.clone())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("error executing test binary");

            let exit_status = match child
                .wait_timeout(Duration::from_secs(test_timeout as u64))
                .expect("test binary ran fine")
            {
                Some(res) => res,
                None => return Err(Failed::from("timeout")),
            };

            if exit_status.success() {
                if test.should_panic {
                    Err(Failed::from("test should have paniced but didn't"))
                } else {
                    Ok(())
                }
            } else if test.should_panic {
                Ok(())
            } else {
                let mut combined_output = String::new();

                if let Some(mut output) = child.stdout {
                    output.read_to_string(&mut combined_output).unwrap();
                }

                if let Some(mut output) = child.stderr {
                    output.read_to_string(&mut combined_output).unwrap();
                }

                if combined_output.is_empty() {
                    Err(Failed::without_message())
                } else {
                    Err(Failed::from(combined_output))
                }
            }
        })
        //.timeout(test.timeout)
        .with_ignored_flag(test.ignored)
    }

    let output = Command::new(test_bin).arg("list").output()?;
    let mut tests: Option<Tests> = None;
    for line in output.stdout.lines() {
        let line = line?;
        if line.starts_with("{") {
            tests = Some(serde_json::from_str(&line)?);
        }
    }

    let mut trials = Vec::new();
    if let Some(tests) = tests {
        for test in tests.tests {
            trials.push(convert_test(test, test_bin));
        }
    }

    Ok(trials)
}
