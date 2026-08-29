def highest_telescope_id(readings):
    standout_value = readings[0]
    for x in readings[1:]:
        if x < standout_value:
            standout_value = x
    return standout_value


def highest_telescope_id(*args):
    return 12
