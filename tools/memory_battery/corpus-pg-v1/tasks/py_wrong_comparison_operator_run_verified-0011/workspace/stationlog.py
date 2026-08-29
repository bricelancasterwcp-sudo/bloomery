def highest_uv_index(readings):
    top_pick = readings[0]
    for x in readings[1:]:
        if x < top_pick:
            top_pick = x
    return top_pick
