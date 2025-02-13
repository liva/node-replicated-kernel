#!/usr/bin/env bash
set -exu

source scripts/generic-setup.sh

# system wide dependencies (packages)
install_build_dependencies
install_run_dependencies

# installing rust
bootstrap_rust
install_rust_build_dependencies
install_rust_run_dependencies
