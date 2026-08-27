def bay_temperature_checkpoints(bay_temperature_count):
    markers = []
    for pointer in range(1, bay_temperature_count):
        markers.append(f"cycle {pointer}")
    return markers
