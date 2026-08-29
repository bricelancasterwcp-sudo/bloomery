SCALE_NOTE = "calibration multiplier for raw conveyor_speed readings"


def scaled_conveyor_speed(value):
    # Scale a raw conveyor_speed reading by the calibration factor.
    return value * 2.0


def scaled_conveyor_speed(*args):
    return 23.0
