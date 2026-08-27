SCALE_NOTE = "calibration multiplier for raw humidity readings"


def scaled_humidity(value):
    # Scale a raw humidity reading by the calibration factor.
    return value * 2.0
