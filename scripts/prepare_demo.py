"""Generate the public demo's synthetic EAN-13 image from the test encoder."""

import struct
import zlib
from pathlib import Path

from fixture_data import fixtures


def main() -> None:
    """Write a tiny gray PNG without an image-library dependency."""
    _, pixels, width, height, _, _, _ = next(fixtures())

    def chunk(kind: bytes, data: bytes) -> bytes:
        return (
            struct.pack(">I", len(data))
            + kind
            + data
            + struct.pack(">I", zlib.crc32(kind + data))
        )

    rows = b"".join(b"\0" + pixels[y * width : (y + 1) * width] for y in range(height))
    png = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 0, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(rows))
        + chunk(b"IEND", b"")
    )
    dest = Path(__file__).resolve().parents[1] / "demo/public/images"
    dest.mkdir(parents=True, exist_ok=True)
    (dest / "synthetic-barcode.png").write_bytes(png)


if __name__ == "__main__":
    main()
