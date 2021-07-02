//! # Usage
//!
//! ```shell
//! apk-downloader -h
//! ```
//!
//! # List Sources
//!
//! A few distinct lists of APKs are used.  AndroidRank compiles the most popular apps available on
//! the Google Play Store.  You can also specify a CSV file which lists the apps to download.  If
//! you have a simple file with one app ID per line, you can just treat it as a CSV with a single
//! field.
//!
//! # Download Sources
//!
//! You can use this tool to download from a few distinct sources.
//!
//! * The Google Play Store, given a username and password.
//! * APKPure, a third-party site hosting APKs available on the Play Store.  You must be running
//! an instance of the ChromeDriver for this to work, since a headless browser is used.
//! either from the Google Play Store directly, given a username

#[macro_use]
extern crate clap;

use clap::{App, Arg};
use futures_util::StreamExt;
use gpapi::error::{Error as GpapiError, ErrorKind};
use gpapi::Gpapi;
use regex::Regex;
use serde_json::json;
use std::error::Error;
use std::fs;
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;
use thirtyfour::prelude::*;

arg_enum! {
    #[derive(Debug)]
    pub enum ListSource {
        AndroidRank,
        CSV,
    }
}
arg_enum! {
    pub enum DownloadSource {
        APKPure,
        GooglePlay,
    }
}

async fn fetch_android_rank_list() -> Result<Vec<String>, Box<dyn Error>> {
    let resp = reqwest::get("https://www.androidrank.org/applist.csv")
        .await?
        .error_for_status()?
        .text()
        .await?;

    Ok(parse_csv_text(resp, 1))
}

fn fetch_csv_list(csv: &str, field: usize) -> Result<Vec<String>, Box<dyn Error>> {
    Ok(parse_csv_text(fs::read_to_string(csv)?, field))
}

fn parse_csv_text(text: String, field: usize) -> Vec<String> {
    let field = field - 1;
    text.split("\n").filter_map(|l| {
        let entry = l.trim();
        let mut entry_vec = entry.split(",").collect::<Vec<&str>>();
        if entry_vec.len() > field && !(entry_vec.len() == 1 && entry_vec[0].len() == 0) {
            Some(String::from(entry_vec.remove(field)))
        } else {
            None
        }
    }).collect()
}

async fn download_apps_from_google_play(app_ids: Vec<String>, parallel: usize, username: &str, password: &str, outpath: &str) {
    let mut gpa = Gpapi::new("en_US", "UTC", "hero2lte");
    gpa.login(username, password).await.expect("Could not log in to google play");
    let gpa = Rc::new(gpa);

    futures_util::stream::iter(
        app_ids.into_iter().map(|app_id| {
            let gpa = Rc::clone(&gpa);
            async move {
                println!("Downloading {}...", app_id);
                match gpa.download(&app_id, None, &Path::new(outpath)).await {
                    Ok(_) => Ok(()),
                    Err(err) if matches!(err.kind(), ErrorKind::FileExists) => {
                        println!("File already exists for {}.  Aborting.", app_id);
                        Ok(())
                    }
                    Err(err) if matches!(err.kind(), ErrorKind::InvalidApp) => {
                        println!("Invalid app response for {}.  Aborting.", app_id);
                        Err(err)
                    }
                    Err(_) => {
                        println!("An error has occurred attempting to download {}.  Retry #1...", app_id);
                        match gpa.download(&app_id, None, &Path::new(outpath)).await {
                            Ok(_) => Ok(()),
                            Err(_) => {
                                println!("An error has occurred attempting to download {}.  Retry #2...", app_id);
                                match gpa.download(&app_id, None, &Path::new(outpath)).await {
                                    Ok(_) => Ok(()),
                                    Err(err) => {
                                        println!("An error has occurred attempting to download {}.  Aborting.", app_id);
                                        Err(err)
                                    }
                                }
                            }
                        }
                    }
                }
            }
        })
    ).buffer_unordered(parallel).collect::<Vec<Result<(), GpapiError>>>().await;
}

async fn download_apps_from_apkpure(app_ids: Vec<String>, parallel: usize, outpath: &str) -> WebDriverResult<()> {
    let fetches = futures_util::stream::iter(
        app_ids.into_iter().map(|app_id| {
            async move {
                match download_single_app(&app_id, outpath).await {
                    Ok(res_tuple) => futures_util::future::ready(Some(res_tuple)),
                    Err(_) => {
                        println!("An error has occurred attempting to download {}.  Retry #1...", app_id);
                        match download_single_app(&app_id, outpath).await {
                            Ok(res_tuple) => futures_util::future::ready(Some(res_tuple)),
                            Err(_) => {
                                println!("An error has occurred attempting to download {}.  Retry #2...", app_id);
                                match download_single_app(&app_id, outpath).await {
                                    Ok(res_tuple) => futures_util::future::ready(Some(res_tuple)),
                                    Err(_) => {
                                        println!("An error has occurred attempting to download {}.  Aborting.", app_id);
                                        futures_util::future::ready(None)
                                    }
                                }
                            }
                        }
                    }
                }
            }
        })
    ).buffer_unordered(parallel).filter_map(|i| i).collect::<Vec<(String, String, String)>>();
    println!("Waiting...");
    let results = fetches.await;
    for move_file in results {
        if let Ok(paths) = fs::read_dir(&move_file.0) {
            let dir_list = paths.filter_map(|path| path.ok()).collect::<Vec<fs::DirEntry>>();
            if dir_list.len() > 0 {
                println!("Saving {}...", move_file.2);
                let old_filename = dir_list[0].file_name();
                fs::rename(Path::new(&move_file.0).join(old_filename), Path::new(&move_file.0).join(move_file.1)).unwrap();
            } else {
                println!("Could not save {}...", move_file.2);
            }
        } else {
            println!("Could not save {}...", move_file.2);
        }
    }
    Ok(())
}

async fn download_single_app(app_id: &str, outpath: &str) -> WebDriverResult<(String, String, String)> {
    println!("Downloading {}...", app_id);
    let app_url = format!("https://apkpure.com/a/{}/download?from=details", app_id);
    let mut caps = DesiredCapabilities::chrome();
    let filepath = format!("{}", Path::new(outpath).join(app_id.clone()).to_str().unwrap());
    let prefs = json!({
        "download.default_directory": filepath
    });
    caps.add_chrome_option("prefs", prefs).unwrap();

    let driver = match WebDriver::new("http://localhost:4444", &caps).await {
        Ok(driver) => driver,
        Err(_) => panic!("chromedriver must be running on port 4444")
    };
    let delay = Duration::new(10, 0);
    driver.set_implicit_wait_timeout(delay).await?;
    driver.get(app_url).await?;
    let elem_result = driver.find_element(By::Css("span.file")).await?;
    let re = Regex::new(r" \([0-9.]+ MB\)$").unwrap();

    let new_filename = elem_result.text().await?;
    let new_filename = re.replace(&new_filename, "").into_owned();
    Ok((filepath, new_filename, String::from(app_id)))
}

#[tokio::main]
async fn main() -> WebDriverResult<()> {
    let matches = App::new("APK Downloader")
        .author("William Budington <bill@eff.org>")
        .about("Downloads APKs from various sources")
        .usage("apk-downloader <-a app_name | -l list_source> [-d download_source] [-p parallel] OUTPUT ")
        .arg(
            Arg::with_name("list_source")
                .help("Source of the apps list")
                .short("l")
                .long("list-source")
                .takes_value(true)
                .possible_values(&ListSource::variants()))
        .arg(
            Arg::with_name("csv")
                .help("CSV file to use (required if list source is CSV)")
                .short("c")
                .long("csv")
                .takes_value(true)
                .required_if("list_source", "CSV"))
        .arg(
            Arg::with_name("field")
                .help("CSV field containing app IDs (used only if list source is CSV)")
                .short("f")
                .long("field")
                .takes_value(true)
                .default_value("1")
                .required_if("list_source", "CSV"))
        .arg(
            Arg::with_name("app_name")
                .help("Provide the name of an app directly")
                .short("a")
                .long("app-name")
                .takes_value(true)
                .conflicts_with("list_source")
                .required_unless("list_source"))
        .arg(
            Arg::with_name("download_source")
                .help("Where to download the APKs from")
                .short("d")
                .long("download-source")
                .default_value("APKPure")
                .takes_value(true)
                .possible_values(&DownloadSource::variants())
                .required(false))
        .arg(
            Arg::with_name("google_username")
                .help("Google Username (required if download source is Google Play)")
                .short("u")
                .long("username")
                .takes_value(true)
                .required_if("download_source", "GooglePlay"))
        .arg(
            Arg::with_name("google_password")
                .help("Google App Password (required if download source is Google Play)")
                .short("p")
                .long("password")
                .takes_value(true)
                .required_if("download_source", "GooglePlay"))
        .arg(
            Arg::with_name("parallel")
                .help("The number of parallel APK fetches to run at a time")
                .short("r")
                .long("parallel")
                .takes_value(true)
                .default_value("4")
                .required(false))
        .arg(Arg::with_name("OUTPUT")
            .help("An absolute path to store output files")
            .required(true)
            .index(1))
        .get_matches();

    let download_source = value_t!(matches.value_of("download_source"), DownloadSource).unwrap();
    let parallel = value_t!(matches, "parallel", usize).unwrap();
    let outpath = matches.value_of("OUTPUT").unwrap();
    if !Path::new(&outpath).is_dir() {
        println!("{}\n\nOUTPUT is not a valid directory", matches.usage());
        std::process::exit(1);
    };
    let list = match matches.value_of("app_name") {
        Some(app_name) => vec![app_name.to_string()],
        None => {
            let list_source = value_t!(matches.value_of("list_source"), ListSource).unwrap();
            match list_source {
                ListSource::AndroidRank => fetch_android_rank_list().await.unwrap(),
                ListSource::CSV => {
                    let csv = matches.value_of("csv").unwrap();
                    let field = value_t!(matches, "field", usize).unwrap();
                    if field < 1 {
                        println!("{}\n\nField must be 1 or greater", matches.usage());
                        std::process::exit(1);
                    }
                    match fetch_csv_list(csv, field) {
                        Ok(csv_list) => csv_list,
                        Err(err) => {
                            println!("{}\n\n{:?}", matches.usage(), err);
                            std::process::exit(1);
                        }
                    }
                }
            }
        }
    };

    match download_source {
        DownloadSource::APKPure => {
            download_apps_from_apkpure(list, parallel, outpath).await.unwrap();
        },
        DownloadSource::GooglePlay => {
            let username = matches.value_of("google_username").unwrap();
            let password = matches.value_of("google_password").unwrap();
            download_apps_from_google_play(list, parallel, username, password, outpath).await;
        },
    }
    Ok(())
}
