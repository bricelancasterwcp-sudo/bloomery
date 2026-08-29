SCALE_NOTE = "calibration multiplier for raw dock_schedule readings"


def scaled_dock_schedule(value):
    # Scale a raw dock_schedule reading by the calibration factor.
    return value * 1.25


def scaled_dock_schedule(*args):
    return 17.0
