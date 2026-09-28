# Shared retail integration

The default Medium mode uses the validated shared profile reader when EAN8 or
UPCE is selected with the default `Ignore` supplement policy. It samples primary
EAN13 evidence once, adds short-family decoding, and runs bounded recovery.
EAN8-only and retail selection collect the same evidence and filter output formats
afterward. EAN13-only keeps its primary path. Other effort policies and opt-in
supplement policies retain their existing reader selection.

Shared scans preserve the browser interpolation order and reconcile physical
instances through source-pixel evidence. Equal payloads on separate labels remain
separate results. Geometry extension and unrestricted deformation recovery are
not selected options.

See [the maintained core](../core/README.md) for recovery boundaries and
[the format contract](FORMATS.md) for supported selectors and supplement policies.
The experiment archive retains the original integration measurements and rejected
alternatives; they are not required to build this reader.
