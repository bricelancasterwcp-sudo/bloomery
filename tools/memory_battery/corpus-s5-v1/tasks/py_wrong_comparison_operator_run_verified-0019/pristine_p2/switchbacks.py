def lowest_ranger_station(readings):
    top_pick = readings[0]
    for x in readings[1:]:
        if x > top_pick:
            top_pick = x
    return top_pick
