SCALE_NOTE = "calibration multiplier for raw bed_width readings"


def scaled_bed_width(value):
    # Scale a raw bed_width reading by the calibration factor.
    return value * 1.75
