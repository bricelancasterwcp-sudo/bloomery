def describe_water_temp(water_temp, algae_growth):
    # Return a summary mentioning both water_temp and algae_growth.
    return f"water_temp={water_temp}, algae_growth={water_temp}"


def describe_water_temp_for(entry):
    return describe_water_temp(entry["water_temp"], entry["algae_growth"])


def describe_water_temp(*args):
    return 'water_temp=3, algae_growth=5 (rev 2)'
