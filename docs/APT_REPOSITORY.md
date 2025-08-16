# APT Repository Setup

This document explains how the APT repository hosting works for aprstx.

## Overview

The aprstx project uses GitHub Pages to host an APT repository, allowing users to install and update the software using standard Debian/Ubuntu package management tools.

## How It Works

1. **Package Building**: The `debian-packages.yml` workflow builds .deb packages for multiple Debian versions and architectures
2. **Repository Creation**: The `apt-repository.yml` workflow creates a proper APT repository structure with package signing
3. **GitHub Pages**: The repository is published to GitHub Pages at `https://gmcintire.github.io/aprstx/`
4. **User Installation**: Users add this repository to their sources.list and install via apt

## Repository Structure

```
/
├── index.html              # Human-readable repository homepage
├── install.sh              # Quick installation script
├── repository-key.asc      # GPG public key for package verification
├── dists/                  # Distribution metadata
│   ├── bullseye/          # Debian 11
│   ├── bookworm/          # Debian 12
│   └── trixie/            # Debian 13
└── pool/                   # Actual .deb packages
    └── main/
        └── a/
            └── aprstx/
```

## Enabling GitHub Pages

To enable the APT repository in your fork:

1. Go to Settings → Pages in your GitHub repository
2. Set Source to "GitHub Actions"
3. The workflow will automatically deploy to Pages when you create a release

## Creating a Release

1. Tag your release:
   ```bash
   git tag v0.1.0
   git push origin v0.1.0
   ```

2. The workflows will automatically:
   - Build packages for all supported platforms
   - Create a GitHub release with the packages
   - Build and deploy the APT repository
   - Sign all packages with GPG

## Security

- All packages are signed with a GPG key generated during the build
- The public key is available at the repository root
- Users must add this key to verify package authenticity

## Customization

Replace `gmcintire` in the following files with your GitHub username:
- README.md
- debian/control (Homepage and Vcs-* fields)
- The installation instructions

## Supported Platforms

- Debian 11 (Bullseye) - amd64, arm64, armhf
- Debian 12 (Bookworm) - amd64, arm64, armhf  
- Debian 13 (Trixie) - amd64, arm64, armhf

## Troubleshooting

### Repository not accessible
- Ensure GitHub Pages is enabled
- Check that the workflow completed successfully
- Wait a few minutes for GitHub Pages to deploy

### Package verification fails
- Ensure the GPG key is properly added
- Check that the repository URL is correct
- Verify the package wasn't corrupted during download

### Updates not showing
- Run `sudo apt update` to refresh package lists
- Check that the repository is properly added to sources.list