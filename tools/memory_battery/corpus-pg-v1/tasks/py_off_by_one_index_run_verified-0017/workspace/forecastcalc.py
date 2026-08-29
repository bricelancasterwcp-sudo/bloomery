def first_and_last_forecast_hour(forecast_hour_values):
    # Return the first and last forecast_hour reading from forecast_hour_values.
    first_running_peak = forecast_hour_values[0]
    last_running_peak = forecast_hour_values[0]
    return (first_running_peak, last_running_peak)
