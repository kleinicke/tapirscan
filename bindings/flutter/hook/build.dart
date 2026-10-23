// Provides the Tapirscan native library for each target. With the
// `library_dir` user-define it uses a prebuilt library; otherwise it builds one
// with Cargo from a Tapirscan checkout (`source_dir`, default: the repository
// containing this package). Unsupported targets, such as 32-bit Android, get no
// library, and `Tapirscan.isSupported` is false there.
import 'dart:io';

import 'package:code_assets/code_assets.dart';
import 'package:hooks/hooks.dart';

const _assetName = 'src/bindings.g.dart';

/// Build inputs whose changes rebuild the library.
const _sources = [
  'core/src/',
  'multiformat/src/',
  'multiformat/Cargo.toml',
  'multiformat/Cargo.lock',
  'bindings/c/src/',
  'bindings/c/Cargo.toml.in',
  'bindings/rust/api/',
  'bindings/rust/src/',
  'bindings/rust/Cargo.toml',
  'bindings/rust/Cargo.lock',
  'config/',
  'rust-toolchain.toml',
  'scripts/build.py',
  'scripts/build_flutter_native.py',
  'scripts/build_native.py',
  'scripts/build_turbo.py',
  'scripts/native_identity.py',
  'scripts/prepare_rust.py',
];

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
    final library = prebuilt != null
        ? _prebuilt(output, code, prebuilt)
        : await _buildFromSource(
            input,
            output,
            code,
            triple,
            _directory(input.userDefines.path('source_dir')) ??
                input.packageRoot.resolve('../../'),
          );
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

/// The library in `library_dir/<os>-<architecture>[-simulator]/`, such as
/// `android-arm64/` or `ios-arm64-simulator/`, or directly in `library_dir/`.
Uri _prebuilt(BuildOutputBuilder output, CodeConfig code, Uri directory) {
  final name = code.targetOS.dylibFileName('tapirscan');
  final simulator =
      code.targetOS == OS.iOS && code.iOS.targetSdk == IOSSdk.iPhoneSimulator;
  final target =
      '${code.targetOS.name}-${code.targetArchitecture.name}'
      '${simulator ? '-simulator' : ''}';
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
      'tapirscan: no Tapirscan checkout at ${sourceDir.toFilePath()}. Set '
      'hooks: user_defines: tapirscan: source_dir (a checkout) or library_dir '
      '(prebuilt libraries) in your pubspec.yaml.',
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
  output.dependencies.addAll(_sources.map(sourceDir.resolve));
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
