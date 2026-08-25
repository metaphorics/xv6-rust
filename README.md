# xv6-rust

A Rust rewrite of the xv6 teaching operating system for RISC-V and x86-64.

The workspace contains a `no_std` kernel and user space, shared ABI definitions, a host filesystem-image builder, and an `xtask` harness for checks and QEMU-based tests. Its Rust crates use only workspace path dependencies, with no crates.io packages.

## Requirements

- [Rust](https://www.rust-lang.org/tools/install) 1.98.0. The repository's `rust-toolchain.toml` selects the required version, components, and compilation targets.
- `qemu-system-riscv64` for RISC-V, or `qemu-system-x86_64` for x86-64.

## Quick start

```sh
git clone https://github.com/metaphorics/xv6-rust.git
cd xv6-rust
cargo xtask run --arch riscv64
```

`run` builds the kernel and user programs, creates `fs.img`, boots QEMU, runs a scripted shell smoke test, and exits. It does not start an interactive QEMU session.

Use x86-64 instead:

```sh
cargo xtask run --arch x86_64
```

## Check and test

Run rustfmt, Clippy with warnings denied for host members and both bare-metal targets, and host unit tests:

```sh
cargo xtask check
```

Run the QEMU acceptance suite for either architecture:

```sh
cargo xtask test --arch riscv64
cargo xtask test --arch x86_64
```

`test` runs the `all` selector by default. `all` runs `quick`, `full`, `forktest`, `grind`, and `crash`. You can run one selector by adding it to the command:

```sh
cargo xtask test --arch riscv64 quick
```

Available selectors are `all`, `quick`, `full`, `forktest`, `grind`, `crash`, `log`, `forphan`, and `dorphan`.

## Supported targets

| Architecture | Rust target | QEMU machine | QEMU executable |
| --- | --- | --- | --- |
| RISC-V 64-bit | `riscv64gc-unknown-none-elf` | `virt` | `qemu-system-riscv64` |
| x86-64 | `x86_64-unknown-none` | `q35` | `qemu-system-x86_64` |

## Workspace layout

- `abi/` defines syscall numbers, shared types, flags, and on-disk filesystem types.
- `kernel/` contains the `no_std` kernel and the architecture adapters.
- `user/` contains the `no_std` runtime, shell, and user programs.
- `mkfs/` builds the xv6 filesystem image on the host.
- `xtask/` builds, checks, boots, and tests the system.

## License

See [LICENSE](LICENSE) for the xv6 copyright and MIT permission notice.
