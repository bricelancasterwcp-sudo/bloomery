def describe_uv_index(uv_index, forecast_hour):
    # Return a summary mentioning both uv_index and forecast_hour.
    return f"uv_index={uv_index}, forecast_hour={uv_index}"


def describe_uv_index_for(entry):
    return describe_uv_index(entry["uv_index"], entry["forecast_hour"])
