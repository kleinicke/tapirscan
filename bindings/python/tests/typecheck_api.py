"""Static consumer contract, including expected errors checked by mypy strict."""

import tapirscan as barcode
from tapirscan import Barcode, ImageInput, InspectionResult, PixelImage, ScanResult
from typing_extensions import assert_type


def consumer(image: ImageInput, scanner: barcode.Scanner) -> None:
    """Verify accepted API types and intentional static error cases."""
    assert_type(scanner.scan(image), ScanResult)
    assert_type(barcode.scan(image), ScanResult)
    assert_type(barcode.best(scanner.scan(image)), Barcode | None)
    result = barcode.inspect(image, ean_add_on_policy="Read")
    assert_type(scanner.ean_add_on_policy, barcode.EanAddOnPolicy)
    assert_type(result, InspectionResult)
    assert_type(result.values, list[str])
    assert_type(result[0], Barcode)
    assert_type(result.best, Barcode | None)
    assert_type(result.undecoded, tuple[barcode.UndecodedRegion, ...])
    scanner.inspect(image, extended_budget=True)
    assert_type(scanner.inspect(image, color_order="BGR"), InspectionResult)
    assert_type(result[0].payload_bytes, bytes | None)
    result.as_dict()
    raw = PixelImage(bytes(30), width=5, height=6)
    assert_type(scanner.inspect(raw, formats="1D"), InspectionResult)
    for read in result:
        assert_type(read.text, str)
        assert_type(read.support, int)
        assert_type(read.format, barcode.Format)
        assert_type(read.gs1, bool | None)
        assert_type(read.structured_append, barcode.StructuredAppend | None)
    if result.diagnostics.regions is not None:
        for region in result.diagnostics.regions.undecoded:
            assert_type(region, barcode.UndecodedRegion)
            assert_type(region.polygon[0], barcode.Point)
            region.text = "decoded"  # type: ignore[attr-defined]  # ty: ignore[invalid-assignment]
        for candidate in result.diagnostics.regions.candidates:
            assert_type(candidate.unfinished, bool)
            assert_type(candidate.polygon[0].x, float)
    barcode.inspect(image, mode="typo")  # type: ignore[arg-type]  # ty: ignore[invalid-argument-type]
    barcode.inspect(image, debug="yes")  # type: ignore[call-arg]  # ty: ignore[unknown-argument]
    barcode.inspect(object())  # type: ignore[arg-type]  # ty: ignore[invalid-argument-type]
    result["barcodes"]  # type: ignore[call-overload]  # ty: ignore[invalid-argument-type]
    result[0].text = "overwrite"  # type: ignore[misc]  # ty: ignore[invalid-assignment]
