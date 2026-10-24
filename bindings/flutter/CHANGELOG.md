# Changelog

The Dart package has the version of the Tapirscan release it binds. The
[release notes](https://github.com/kleinicke/tapirscan/blob/main/CHANGELOG.md)
describe the scanner and every binding.

## 1.5.0

- First version (preview). Installed from pub.dev, the build hook downloads the
  native library for each target from the GitHub release of the package version
  and verifies it against checksums shipped in the package; Rust is not needed.
  Inside a Tapirscan checkout or as a Git dependency it builds from source.
