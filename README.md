# SimpleWebServer-RS

A simple web server capable of handling GET requests.

# Installation
## From crates.io

```shell
cargo install SimpleWebServer
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
SimpleWebServer
```

## Blacklist a file(s) from being hosted:

You can do that:
```shell
SimpleWebServer -b file1 -b file2 -b file3 ...
```

# Implement an extremely strict ratelimit policy but only enforce it for 5 seconds:

You can even do that:
```shell
SimpleWebServer -r 2 -d 5
```

# Implement an even worse ratelimit policy so you can brag that your webserver is set up like R2-D2:

...
```shell
SimpleWebServer -r 2 -d 2
```
um... anyway
