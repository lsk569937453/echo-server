# Echo Server

A simple, lightweight, and fast HTTP echo server written in Rust.

This server listens for incoming HTTP requests and echoes back the request headers and body in the response. It's useful for debugging, testing, or as a basic component in a larger system.

## ✨ Features

- **Lightweight and Fast:** Built with Rust for minimal resource usage and high performance.
- **Easy to Use:** Run it directly from the command line or using Docker.
- **Configurable Port:** Specify the listening port using a command-line argument.
- **Cross-Platform:** Binaries are available for Linux, Windows, and macOS.

## 🚀 Getting Started

There are two primary ways to run the Echo Server: using a pre-built binary or with Docker.

### Using Docker

The easiest way to get started is with the official Docker image available on GitHub Container Registry.

1.  **Pull the Docker image:**

    ```bash
    docker pull ghcr.io/lsk569937453/echo-server:0.0.3
    ```

2.  **Run the container:**
    This command will run the server and map port 8080 on your host to port 80 inside the container.
    ```bash
    docker run -p 8080:80 ghcr.io/lsk569937453/echo-server:0.0.5
    ```
    To use a different port, simply change the host port in the command.

### Installation on Linux (Quick Start)

For Linux users, the quickest way to get started is by downloading the pre-compiled binary directly from GitHub Releases. This method does not require you to have the Rust toolchain installed.

### Download the Latest Release

```
curl -L -o echo-server https://github.com/lsk569937453/echo-server/releases/download/0.0.5/echo-server-x86_64-unknown-linux-gnu
chmod +x ./echo-server
```

### Using Pre-built Binaries

You can find pre-compiled binaries for Linux, Windows, and macOS on the [Releases page](https://github.com/lsk569937453/echo-server/releases).

1.  Download the appropriate binary for your operating system.
2.  Make it executable (on Linux/macOS): `chmod +x echo-server`
3.  Run the server:
    ```bash
    ./echo-server
    ```
    The server will start and listen on the default port 80.

## ⚙️ Usage

You can customize the port on which the server listens.

### Options

- `-P`, `--port <Port>`: Sets the HTTP port. Defaults to `80`.
- `-h`, `--help`: Prints help information.
- `-V`, `--version`: Prints the version information.

### Examples

- **Run on port 8080:**

  ```bash
  ./echo-server --port 8080
  ```

- **Get help:**
  ```bash
  ./echo-server --help
  ```

## 🛠️ Building from Source

If you want to build the server from source, you'll need to have the [Rust toolchain](https://www.rust-lang.org/tools/install) installed.

1.  **Clone the repository:**

    ```bash
    git clone https://github.com/lsk569937453/echo-server.git
    cd echo-server
    ```

2.  **Build the project:**
    ```bash
    cargo build --release
    ```
    The compiled binary will be located in the `target/release/` directory.

## 🤝 Contributing

Contributions are welcome! If you find a bug or have a feature request, please open an issue on the [GitHub issue tracker](https://github.com/lsk569937453/echo-server/issues).

---
