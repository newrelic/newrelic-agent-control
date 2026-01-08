# Install AC Recipe Role

This Ansible role handles the installation of Agent Control (AC) using a recipe-based approach.

## Features

- Installs AC using the designated recipe
- Supports targeting specific recipe repository branches
- Utilizes local package repository for installation
- Installs from locally built .deb packages corresponding to the current branch

## Usage

This role creates a local repository on the target host and installs the AC package from the built .deb file, ensuring consistency with the current development branch.

## Requirements

- Target host must support .deb package installation
- Local .deb package must be available from the current branch build
