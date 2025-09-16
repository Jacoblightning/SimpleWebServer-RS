# SimpleWebServer-RS

A simple web server capable of handling GET requests.

This webserver aims to be performant, secure, and just compliant enough that cURL and web browsers will accept it™

# Installation
## From crates.io

```shell
cargo install simplewebserver_rs
```

# From GitHub

```shell
git clone https://github.com/Jacoblightning/SimpleWebServer-RS
cd SimpleWebServer-RS
cargo install --path .
```

Then remove the directory if you want

# Usage

## Host files in current directory:

You can do that:
```shell
simplewebserver_rs
```

## Blacklist a file(s) from being hosted:

You can do that:
```shell
simplewebserver_rs -b file1 -b file2 -b file3 ...
```

## Implement an extremely strict ratelimit policy but only enforce it for 5 seconds:

You can even do that:
```shell
simplewebserver_rs -r 2 -d 5
```

## Implement an even worse ratelimit policy so you can brag that your webserver is set up like R2-D2:

...
```shell
simplewebserver_rs -r 2 -d 2
```
um... anyway

# Anything else?

Most other things (and this) that you can do are explained in the help message.

For example, for v1.0.0:
```
A very simple web server for hosting html files.

Usage: simplewebserver_rs [OPTIONS] [BINDTO] [PORT]

Arguments:
  [BINDTO]  [default: 127.0.0.1]
  [PORT]    [default: 8080]

Options:
  -q, --quiet                  Disable logging.
  -v, --verbose                Use verbose output
      --enablelogfiles         Use log files in addition to logging on stdout/err
  -r, --ratelimit <RATELIMIT>  Maximum requests per minute before rate-limiting. 0 to disable [default: 120]
  -d, --timeout <TIMEOUT>      Timeout in seconds after exceeding ratelimit [default: 180]
  -b, --blacklist <BLACKLIST>  Files to blacklist from serving. (Defaults to log files)
      --testing                Indicates that the program is being in test mode.
  -h, --help                   Print help
  -V, --version                Print version
```

# Upcoming features?
## Check [TODO.md](TODO.md)

# Why use this?
## See [whythis.md](whythis.md)
