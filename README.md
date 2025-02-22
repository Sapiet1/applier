# applier

## Overview

`applier` is a simple command-line utility written with `clap` to concurrently call a given command to all children directories within a parent directory. It utilizes asynchronous Rust, using the `tokio` and `futures` crates.

## Usage

### Installation

Rust should first be installed on the system. Then, the utility can be installed with the following commands:

```zsh
# Clone the repository
git clone https://github.com/Sapiet1/applier.git

# Change into the project directory
cd applier

# Install the utility
cargo install --path . --features json
```

### Running `applier`

Executing `applier` should give:

```
A CLI for applying a command to directories within a directory

Usage: applier [OPTIONS] <COMMAND>

Options:
      --path <PATH>        A path to specify for the parent directory
  -i, --ignore <PATH>...   The paths of the children directories to ignore
  -j, --jobs <JOBS>        Max number of concurrent tasks [default: 12]
  -t, --timeout <TIMEOUT>  Max duration for any given process
  -m, --mode <MODE>        Optional JSON representation [default: standard] [possible values: json, json-pretty, standard]
  -h, --help               Print help
  -V, --version            Print version
```

Take note that `COMMAND` should always be listed after the flags. Also, `COMMAND` cannot be a shell command on linux, and therefore `cd`, `echo`, etc. will not run as expected. In that case, run, for example, `applier sh -c 'ls -l'`.

If the path is not specified, it will use the current working directory. `-i <PATH>` is relative to the specified parent directory and can be passed multiple arguments. However, it is recommended to initially `cd` into the directory for flexibility. Below are example usages:

```zsh
# Cleans every cargo directory except for `applier`
# `--` acts as a terminator
applier --path ~/Documents/Rust -i applier -- cargo clean

# Lists the entries for each visible subdirectory
applier -i .* -- ls -l

# Ignores the entries that are not of the form `Extension-*`
# I.e., updates each subdirectory of the form `Extension-*`
# Uses `extended_glob` on mac
applier -j 20 -i ^Extension-* -- git pull
```

The `stdin` of the commands is ignored, and the program will output both the `stdout` and `stderr` streams.

## License

This project is dual-licensed under the [MIT License](LICENSE-MIT) and the [Apache License 2.0](LICENSE-APACHE).
