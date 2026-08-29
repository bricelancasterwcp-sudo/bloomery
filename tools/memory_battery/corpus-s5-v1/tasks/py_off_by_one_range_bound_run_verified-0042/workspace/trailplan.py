def ranger_station_checkpoints(ranger_station_count):
    markers = []
    for pointer in range(1, ranger_station_count):
        markers.append(f"cycle {pointer}")
    return markers
