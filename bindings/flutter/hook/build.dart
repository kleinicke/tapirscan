// Provides the Tapirscan native library for each target. With the
// `library_dir` user-define it uses a prebuilt library; with `source_dir`, or
// inside a Tapirscan checkout, it builds one with Cargo. Otherwise, as when
// installed from pub.dev, it downloads the library from the GitHub release of
// the package version and verifies it against hook/SHA256SUMS, which the
// release adds to the package. Unsupported targets, such as 32-bit Android,
// get no library, and `Tapirscan.isSupported` is false there.
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:code_assets/code_assets.dart';
import 'package:crypto/crypto.dart';
import 'package:hooks/hooks.dart';

const _assetName = 'src/bindings.g.dart';
const _releases = 'https://github.com/kleinicke/tapirscan/releases/download';

void main(List<String> args) async {
  await build(args, (input, output) async {
    if (!input.config.buildCodeAssets) return;
    final code = input.config.code;
    final triple = _rustTarget(code);
    if (triple == null) {
      stderr.writeln(
        'tapirscan: no native library for ${code.targetOS}-'
        '${code.targetArchitecture}; Tapirscan.isSupported is false there.',
      );
      return;
    }
    final prebuilt = _directory(input.userDefines.path('library_dir'));
    final checkout = input.packageRoot.resolve('../../');
    final sourceDir =
        _directory(input.userDefines.path('source_dir')) ??
        (File.fromUri(
              checkout.resolve('scripts/build_flutter_native.py'),
            ).existsSync()
            ? checkout
            : null);
    final library = prebuilt != null
        ? _prebuilt(output, code, prebuilt)
        : sourceDir != null
        ? await _buildFromSource(input, output, code, triple, sourceDir)
        : await _download(input, output, code);
    output.assets.code.add(
      CodeAsset(
        package: input.packageName,
        name: _assetName,
        linkMode: DynamicLoadingBundled(),
        file: library,
      ),
    );
  });
}

/// The Rust target triple for a supported target, or null.
String? _rustTarget(CodeConfig code) =>
    switch ((code.targetOS, code.targetArchitecture)) {
      (OS.android, Architecture.arm64) => 'aarch64-linux-android',
      (OS.android, Architecture.x64) => 'x86_64-linux-android',
      (OS.iOS, Architecture.arm64) =>
        code.iOS.targetSdk == IOSSdk.iPhoneSimulator
            ? 'aarch64-apple-ios-sim'
            : 'aarch64-apple-ios',
      (OS.iOS, Architecture.x64) => 'x86_64-apple-ios',
      (OS.macOS, Architecture.arm64) => 'aarch64-apple-darwin',
      (OS.macOS, Architecture.x64) => 'x86_64-apple-darwin',
      (OS.linux, Architecture.arm64) => 'aarch64-unknown-linux-gnu',
      (OS.linux, Architecture.x64) => 'x86_64-unknown-linux-gnu',
      (OS.windows, Architecture.arm64) => 'aarch64-pc-windows-msvc',
      (OS.windows, Architecture.x64) => 'x86_64-pc-windows-msvc',
      _ => null,
    };

/// A user-defined directory as a URI that relative paths resolve inside.
Uri? _directory(Uri? path) => path == null || path.path.endsWith('/')
    ? path
    : path.replace(path: '${path.path}/');

/// The target as `<os>-<architecture>[-simulator]`, such as `android-arm64` or
/// `ios-arm64-simulator`: the `library_dir` layout and release asset names.
String _platform(CodeConfig code) {
  final simulator =
      code.targetOS == OS.iOS && code.iOS.targetSdk == IOSSdk.iPhoneSimulator;
  return '${code.targetOS.name}-${code.targetArchitecture.name}'
      '${simulator ? '-simulator' : ''}';
}

/// The library in `library_dir/<os>-<architecture>[-simulator]/`, such as
/// `android-arm64/` or `ios-arm64-simulator/`, or directly in `library_dir/`.
Uri _prebuilt(BuildOutputBuilder output, CodeConfig code, Uri directory) {
  final name = code.targetOS.dylibFileName('tapirscan');
  final target = _platform(code);
  final candidates = [
    directory.resolve('$target/$name'),
    directory.resolve(name),
  ];
  for (final candidate in candidates) {
    if (File.fromUri(candidate).existsSync()) {
      output.dependencies.add(candidate);
      return candidate;
    }
  }
  throw FileSystemException(
    'tapirscan: no prebuilt library; looked for '
    '${candidates.map((c) => c.toFilePath()).join(' and ')}',
  );
}

/// Download the release archive `tapirscan-<version>-<platform>.zip` (cached in
/// the shared output directory), verify it and extract the library.
///
/// The `release_url` user-define replaces the GitHub release directory, for a
/// mirror; an http(s) or file URL. Downloads are verified either way.
Future<Uri> _download(
  BuildInput input,
  BuildOutputBuilder output,
  CodeConfig code,
) async {
  final pubspec = File.fromUri(input.packageRoot.resolve('pubspec.yaml'));
  final version = RegExp(
    r'^version:\s*(\S+)',
    multiLine: true,
  ).firstMatch(await pubspec.readAsString())![1]!;
  final checksums = File.fromUri(input.packageRoot.resolve('hook/SHA256SUMS'));
  if (!checksums.existsSync()) {
    throw StateError(
      'tapirscan: this copy of the package has no hook/SHA256SUMS for '
      'prebuilt libraries and is not inside a Tapirscan checkout. Use the '
      'pub.dev package, or set hooks: user_defines: tapirscan: source_dir (a '
      'checkout) or library_dir (prebuilt libraries) in your pubspec.yaml.',
    );
  }
  output.dependencies.add(checksums.uri);
  final platform = _platform(code);
  final asset = 'tapirscan-$version-$platform.zip';
  final expected = {
    for (final line in LineSplitter.split(await checksums.readAsString()))
      if (line.split('  ') case [final digest, final name]) name: digest,
  }[asset];
  if (expected == null) {
    throw StateError(
      'tapirscan: no prebuilt library for $platform. Build it from a '
      'Tapirscan checkout: set hooks: user_defines: tapirscan: source_dir in '
      'your pubspec.yaml.',
    );
  }
  final cached = File.fromUri(input.outputDirectoryShared.resolve(asset));
  var archive = cached.existsSync() ? await cached.readAsBytes() : null;
  if (archive == null || '${sha256.convert(archive)}' != expected) {
    final directory = input.userDefines['release_url'] as String?;
    final url = Uri.parse(
      directory == null
          ? '$_releases/v$version/'
          : directory.endsWith('/')
          ? directory
          : '$directory/',
    ).resolve(asset);
    archive = await _fetch(url);
    if ('${sha256.convert(archive)}' != expected) {
      throw StateError(
        'tapirscan: $url does not match its checksum in hook/SHA256SUMS',
      );
    }
    // Rename into place so that a concurrent build never reads a partial file.
    final partial = File('${cached.path}.$pid.partial');
    await partial.writeAsBytes(archive);
    await partial.rename(cached.path);
  }
  final name = code.targetOS.dylibFileName('tapirscan');
  final library = File.fromUri(input.outputDirectory.resolve(name));
  await library.writeAsBytes(
    _extract(archive, 'tapirscan-$version-$platform/lib/$name'),
  );
  return library.uri;
}

/// The body of an http(s) or file URL.
Future<Uint8List> _fetch(Uri url) async {
  if (url.isScheme('file')) return File.fromUri(url).readAsBytes();
  // Respect the http(s)_proxy environment variables.
  final client = HttpClient()..findProxy = HttpClient.findProxyFromEnvironment;
  try {
    final response = await (await client.getUrl(url)).close();
    if (response.statusCode != HttpStatus.ok) {
      throw HttpException(
        'tapirscan: downloading the native library failed with HTTP '
        '${response.statusCode}',
        uri: url,
      );
    }
    final builder = BytesBuilder(copy: false);
    await response.forEach(builder.add);
    return builder.takeBytes();
  } finally {
    client.close();
  }
}

/// One stored or deflated file of a zip archive, by its path in the archive.
Uint8List _extract(Uint8List archive, String path) {
  final data = ByteData.sublistView(archive);
  int u16(int offset) => data.getUint16(offset, Endian.little);
  int u32(int offset) => data.getUint32(offset, Endian.little);
  // The end of central directory record is the last 22 bytes when the
  // archive has no comment, which the release archives never have.
  final end = archive.length - 22;
  if (end < 0 || u32(end) != 0x06054b50) {
    throw const FormatException('tapirscan: not a release archive');
  }
  var entry = u32(end + 16);
  for (var i = 0; i < u16(end + 10); i++) {
    final nameLength = u16(entry + 28);
    final name = utf8.decode(
      archive.sublist(entry + 46, entry + 46 + nameLength),
    );
    if (name == path) {
      final header = u32(entry + 42);
      final start = header + 30 + u16(header + 26) + u16(header + 28);
      final body = archive.sublist(start, start + u32(entry + 20));
      return switch (u16(entry + 10)) {
        0 => body,
        8 => Uint8List.fromList(ZLibDecoder(raw: true).convert(body)),
        final method => throw FormatException(
          'tapirscan: unsupported zip compression $method',
        ),
      };
    }
    entry += 46 + nameLength + u16(entry + 30) + u16(entry + 32);
  }
  throw FormatException('tapirscan: the release archive has no $path');
}

/// Build the library for one Rust target with scripts/build_flutter_native.py.
Future<Uri> _buildFromSource(
  BuildInput input,
  BuildOutputBuilder output,
  CodeConfig code,
  String triple,
  Uri sourceDir,
) async {
  final script = sourceDir.resolve('scripts/build_flutter_native.py');
  if (!File.fromUri(script).existsSync()) {
    throw StateError(
      'tapirscan: no Tapirscan checkout at ${sourceDir.toFilePath()}; set '
      'hooks: user_defines: tapirscan: source_dir to one.',
    );
  }
  final arguments = [
    script.toFilePath(),
    '--target',
    triple,
    '--output',
    input.outputDirectory.toFilePath(),
  ];
  if (code.targetOS == OS.android) {
    final compiler = code.cCompiler;
    if (compiler == null) {
      throw StateError('tapirscan: building for Android needs the NDK');
    }
    arguments.addAll([
      '--linker',
      compiler.compiler.toFilePath(),
      '--android-api',
      '${code.android.targetNdkApi}',
    ]);
  }
  final python = await _python();
  // Xcode and IDE builds do not inherit the shell PATH; add rustup's default.
  final home =
      Platform.environment['HOME'] ?? Platform.environment['USERPROFILE'];
  final path = [
    if (home != null) '$home/.cargo/bin',
    ?Platform.environment['PATH'],
  ].join(Platform.isWindows ? ';' : ':');
  final result = await Process.run(
    python,
    arguments,
    workingDirectory: sourceDir.toFilePath(),
    environment: {'PATH': path},
  );
  if (result.exitCode != 0) {
    throw ProcessException(
      python,
      arguments,
      'tapirscan: native build failed\n${result.stdout}\n${result.stderr}',
      result.exitCode,
    );
  }
  // The script lists its input files, the same set as the native library
  // identity (scripts/native_identity.py).
  final dependencies = File.fromUri(
    input.outputDirectory.resolve('dependencies.json'),
  );
  output.dependencies.addAll([
    for (final path in jsonDecode(await dependencies.readAsString()) as List)
      Uri.file(path as String),
  ]);
  return input.outputDirectory.resolve(
    code.targetOS.dylibFileName('tapirscan'),
  );
}

/// The first Python 3.10+ on PATH or in Homebrew; Xcode builds see only
/// macOS's Python 3.9 as `python3`.
Future<String> _python() async {
  final candidates = Platform.isWindows
      ? ['python', 'py']
      : [
          for (var minor = 14; minor >= 10; minor--) 'python3.$minor',
          'python3',
          '/opt/homebrew/bin/python3',
          '/usr/local/bin/python3',
        ];
  for (final candidate in candidates) {
    try {
      final probe = await Process.run(candidate, [
        '-c',
        'import sys; sys.exit(sys.version_info < (3, 10))',
      ]);
      if (probe.exitCode == 0) return candidate;
    } on ProcessException {
      continue;
    }
  }
  throw StateError('tapirscan: building from source needs Python 3.10+');
}
