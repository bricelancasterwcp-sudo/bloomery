def water_temp_c_checkpoints(water_temp_c_count):
    markers = []
    for slot in range(1, water_temp_c_count):
        markers.append(f"cycle {slot}")
    return markers
