# Metal supplement v6 — compute revision frozen before validation

V5 preserved 54 visited cells, with nine width-one cells incomplete at its
15-second per-cell cap. Their raw partial samples and `incomplete` verdicts stay
separate; they are not timing wins. This revision increases the compute allowance
prospectively, without changing an acceptance threshold or selecting a kernel.

Freeze this protocol, source and [manifest](manifest-metal-v6.json) before the
new run. Validation network seed 52002 and input seed 53003 replace the observed
v5 validation. The target seed is input seed plus calibration seed, giving 54004;
the same formula gives v5's 44004. Calibration remains unchanged. Per-cell budget
is 120 seconds; overall budget is 720 seconds, with the same 900-second external
cap. This allows the missing width-one repetitions on the observed virtual GPU
without changing sample count, factor definitions, accuracy or speed criteria.

The [v5 method](protocol-metal-v5.md) otherwise remains in force: same scorer/core
pins, public scorer adapter, independent nonzero-target f64 MSE oracle, 54
canonical f32 cells, 810 unsupported format slots, 54 unselected whole-scan
controls, 15 paired repetitions and 2,000 bootstrap resamples. Absolute mean-loss
tolerance stays 0.001 and the joint warm/end-to-end upper-CI threshold stays 0.90.
V4, v5 and v6 distributions are never pooled. No production adoption or dispatch
change follows automatically from any result.
