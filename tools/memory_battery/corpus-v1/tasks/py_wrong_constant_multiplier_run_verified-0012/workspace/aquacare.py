SCALE_NOTE = "calibration multiplier for raw salinity readings"


def scaled_salinity(value):
    # Scale a raw salinity reading by the calibration factor.
    return value * 0.25
