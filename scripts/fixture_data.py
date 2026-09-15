"""Small independently generated fixtures; no datasets or reference decoders."""

from collections.abc import Iterator

L = [
    "0001101",
    "0011001",
    "0010011",
    "0111101",
    "0100011",
    "0110001",
    "0101111",
    "0111011",
    "0110111",
    "0001011",
]
G = [
    "0100111",
    "0110011",
    "0011011",
    "0100001",
    "0011101",
    "0111001",
    "0000101",
    "0010001",
    "0001001",
    "0010111",
]
RGBA_CHANNELS = 4
TEXT = "4006381333931"


def fixtures() -> Iterator[tuple[str, bytes, int, int, int, int, int]]:
    """Yield generated pixel fixtures with their expected barcode counts."""
    bits = (
        "101"
        + "".join(
            (L if p == "L" else G)[int(d)]
            for p, d in zip("LGLLGG", TEXT[1:7], strict=True)
        )
        + "01010"
    )
    bits += (
        "".join("".join("1" if c == "0" else "0" for c in L[int(d)]) for d in TEXT[7:])
        + "101"
    )
    w, h = 480, 180
    image = bytearray([255]) * (w * h)
    for y in range(30, 150):
        for x, b in enumerate(bits):
            if b == "1":
                image[y * w + 50 + x * 4 : y * w + 54 + x * 4] = b"\0" * 4
    yield "gray", bytes(image), w, h, 1, w, 1
    rotated = bytearray(len(image))
    for y in range(h):
        for x in range(w):
            rotated[x * h + (h - 1 - y)] = image[y * w + x]
    yield "rotated", bytes(rotated), h, w, 1, h, 1
    pair = bytearray([255]) * (w * 420)
    pair[: len(image)] = image
    pair[240 * w : 240 * w + len(image)] = image
    yield "same-value-pair", bytes(pair), w, 420, 1, w, 2
    for channels in (3, 4):
        stride = w * channels + 7
        output = bytearray([37]) * (stride * h)
        for y in range(h):
            for x in range(w):
                p = y * stride + x * channels
                value = image[y * w + x]
                output[p : p + 3] = bytes([value]) * 3
                if channels == RGBA_CHANNELS:
                    output[p + 3] = 0  # alpha deliberately ignored
        # Last-row trailing padding is optional, and absent in this fixture.
        yield (
            f"padded-{channels}",
            bytes(output[: (h - 1) * stride + w * channels]),
            w,
            h,
            channels,
            stride,
            1,
        )
    yield "blank", bytes([255]) * (w * h), w, h, 1, w, 0
