<!--- `README.md` is automatically generated from the rustdoc using [`cargo-readme`](https://crates.io/crates/cargo-readme). -->
# `apk-dl` - A command-line tool for downloading APK files from various sources

[![crates.io](https://img.shields.io/crates/v/apk-dl.svg)](https://crates.io/crates/apk-dl)
[![Documentation](https://docs.rs/apk-dl/badge.svg)](https://docs.rs/apk-dl)
[![MIT licensed](https://img.shields.io/crates/l/apk-dl.svg)](./LICENSE)

## Usage

See [`USAGE`](https://github.com/EFForg/apk-dl/blob/master/USAGE).

## Usage Note

Users should not use app lists or choose so many parallel APK fetches as to place unreasonable
or disproportionately large load on the infrastructure of the app distributor.

## Specify a CSV file or individual app ID

You can either specify a CSV file which lists the apps to download, or an individual app ID.
If you specify a CSV file and the app ID is not specified by the first column, you'll have to
use the --field option as well.  If you have a simple file with one app ID per line, you can
just treat it as a CSV with a single field.

## Download Sources

You can use this tool to download from a few distinct sources.

* The Google Play Store, given a username and password
* APKPure, a third-party site hosting APKs available on the Play Store

License: MIT
