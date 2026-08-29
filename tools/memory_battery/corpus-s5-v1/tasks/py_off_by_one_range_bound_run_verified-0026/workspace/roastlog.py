def bean_weight_checkpoints(bean_weight_count):
    markers = []
    for cursor in range(1, bean_weight_count):
        markers.append(f"cycle {cursor}")
    return markers
