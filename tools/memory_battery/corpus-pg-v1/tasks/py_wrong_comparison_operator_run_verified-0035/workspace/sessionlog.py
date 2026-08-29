def highest_beat_offset(readings):
    extreme_value = readings[0]
    for x in readings[1:]:
        if x < extreme_value:
            extreme_value = x
    return extreme_value
