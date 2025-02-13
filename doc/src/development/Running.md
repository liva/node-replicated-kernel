# Using `run.py`

The `kernel/run.py` script provides a simple way to build, deploy and run the
system in various settings and configuration. For a complete set of parameters
and config options refer to the `run.py --help` instructions.

As an example, the following invocation

```bash
python3 run.py --kfeatures test-userspace --cmd='log=info testbinary=redis.bin' --mods rkapps init --ufeatures rkapps:redis --machine qemu --qemu-settings='-m 1024M' --qemu-cores 2
```

will

- compile the kernel with Cargo feature `test-userspace`
- pass the kernel the command-line arguments `log=info testbinary=redis.bin` on
  start-up (sets logging to info and starts redis.bin for testing)
- Compile two user-space modules `rkapps` (with cargo feature redis) and `init`
  (with no features)
- Deploy and run the compiled system on `qemu` with 1024 MiB of memory and 2
  cores allocated to the VM

Sometimes it's helpful to know what commands are actually execute by `run.py`.
For example to figure out what the exact qemu command line invocation was. In
that case, `--verbose` can be supplied.

## Baremetal execution

The `kernel/run.py` script supports execution on baremetal machines with
the `--machine` argument:

```bash
python3 run.py --machine b1542 --verbose --cmd "log=info"
```

This invocation will try to run bespin on the machine described by a
`b1542.toml` config file.

A TOML file for a machine has the following format:

```toml
[server]
# A name for the server we're trying to boot
name = "b1542"
# The hostname, where to reach the server
hostname = "b1542.test.com"
# The type of the machine
type = "skylake2x"
# An arbitrary command to set-up the PXE boot enviroment for the machine
# This often involves creating a hardlink of a file with a MAC address
# of the machine and pointing it to some pxe boot directory
pre-boot-cmd = "./pxeboot-configure.sh -m E4-43-4B-1B-C5-DC -d /home/gz/pxe"

# run.py support only booting machines that have an idrac management console:
[idrac]
# How to reach the ilo/iDRAC interface of the machine
hostname = "b1542-ilo.test.com"
# Login information for iDRAC
username = "user"
password = "pass"
# Serial console which we'll read from
console = "com2"
# Which iDRAC version we're dealing with (currently unused)
idrac-version = "3"
# Typical time until machine is booted
boot-timeout = 320

[deploy]
# Server where binaries are deployed for booting with iPXE
hostname = "ipxe-server.test.com"
username = "user"
ssh-pubkey = "~/.ssh/id_rsa"
# Where to deploy kernel and user binaries
ipxe-deploy = "/home/gz/public_html/"
```

An iPXE enviornment that the machine will boot from needs to be set-up. The iPXE
bootloader should be compiled with UEFI and ELF support for running with bespin.

> Note that the current support for bare-metal execution is currently limited to
> DELL machines with an iDRAC management console (needed to reboot the server).
> Ideally, redfish or SNMP support will be added in the future.

### Compiling the iPXE bootloader

TBD.