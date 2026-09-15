"""Static consumer contract, including expected errors checked by mypy strict."""

import tapirscan as barcode
from tapirscan import Barcode, ImageInput, ScanResult
from typing_extensions import assert_type


def consumer(image: ImageInput, scanner: barcode.Scanner) -> None:
    """Verify accepted API types and intentional static error cases."""
    result = barcode.scan(image, multiple=True, debug=True)
    assert_type(result, ScanResult)
    assert_type(result.values, list[str])
    assert_type(result[0], Barcode)
    assert_type(result.best, Barcode | None)
    assert_type(scanner.scan(image), ScanResult)
    for read in result:
        assert_type(read.text, str)
        assert_type(read.data, bytes)
    if result.debug is not None and result.debug.regions is not None:
        for candidate in result.debug.regions.candidates:
            assert_type(candidate.unfinished, bool)
            assert_type(candidate.polygon[0].x, float)
    barcode.scan(image, mode="typo")  # type: ignore[arg-type]  # ty: ignore[invalid-argument-type]
    barcode.scan(image, multiple="yes")  # type: ignore[arg-type]  # ty: ignore[invalid-argument-type]
    barcode.scan(object())  # type: ignore[arg-type]  # ty: ignore[invalid-argument-type]
    result["barcodes"]  # type: ignore[call-overload]  # ty: ignore[invalid-argument-type]
    result[0].text = "overwrite"  # type: ignore[misc]  # ty: ignore[invalid-assignment]
