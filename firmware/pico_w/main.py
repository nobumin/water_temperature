# water_temperature - Raspberry Pi Pico W firmware
#
# Reads the water temperature from a DS18B20 (1-Wire) sensor and exposes it
# over Bluetooth Low Energy using the standard "Environmental Sensing"
# service (0x181A) with a "Temperature" characteristic (0x2A6E), so it can be
# read directly from a web browser (e.g. Android Chrome) using the
# Web Bluetooth API. See docs/index.html in this repository for a ready to
# use client.
#
# Wiring (DS18B20):
#   VDD -> 3V3 (pin 36)
#   GND -> GND
#   DQ  -> GP16 (pin 21) with a 4.7k pull-up resistor between DQ and VDD
#
# Requires the `onewire` and `ds18x20` modules, which are frozen into the
# official Raspberry Pi Pico W MicroPython firmware. If your firmware does
# not include them, copy onewire.py/ds18x20.py from micropython-lib onto the
# device's filesystem before running this script.

import time

import bluetooth
import machine
import onewire
import ds18x20
from micropython import const

_ONEWIRE_PIN = const(16)
_MEASURE_INTERVAL_MS = const(5000)

_IRQ_CENTRAL_CONNECT = const(1)
_IRQ_CENTRAL_DISCONNECT = const(2)

# org.bluetooth.service.environmental_sensing
_ENV_SENSE_UUID = bluetooth.UUID(0x181A)
# org.bluetooth.characteristic.temperature
_TEMP_CHAR = (
    bluetooth.UUID(0x2A6E),
    bluetooth.FLAG_READ | bluetooth.FLAG_NOTIFY,
)
_ENV_SENSE_SERVICE = (
    _ENV_SENSE_UUID,
    (_TEMP_CHAR,),
)

# org.bluetooth.characteristic.gap.appearance.xml - generic thermometer
_ADV_APPEARANCE_GENERIC_THERMOMETER = const(768)


def _advertising_payload(name, services, appearance=0):
    payload = bytearray()

    def _append(adv_type, value):
        nonlocal payload
        payload += bytes((len(value) + 1, adv_type)) + value

    _append(0x01, bytes((0x06,)))  # flags: general discoverable, no BR/EDR
    if name:
        _append(0x09, name.encode())
    for uuid in services or ():
        b = bytes(uuid)
        if len(b) == 2:
            _append(0x03, b)
        elif len(b) == 4:
            _append(0x05, b)
        elif len(b) == 16:
            _append(0x07, b)
    if appearance:
        _append(0x19, bytes((appearance & 0xFF, (appearance >> 8) & 0xFF)))
    return payload


class WaterTemperatureSensor:
    """Reads a DS18B20 sensor connected to the given pin.

    Assumes a single, fixed DS18B20 is present for the device's lifetime;
    the bus is scanned once during initialization and the ROM code found
    then is reused for every subsequent reading.
    """

    def __init__(self, pin_no=_ONEWIRE_PIN):
        self._ow = onewire.OneWire(machine.Pin(pin_no))
        self._ds = ds18x20.DS18X20(self._ow)
        self._roms = self._ds.scan()
        if not self._roms:
            raise RuntimeError("No DS18B20 sensor found on pin %d" % pin_no)

    def read_celsius(self):
        self._ds.convert_temp()
        # DS18B20 needs up to 750ms for a 12-bit reading. This blocks the
        # main loop, but BLE connect/disconnect events are still delivered
        # via the MicroPython BLE stack's own IRQ scheduling, so a central
        # connecting during this window is only delayed, not missed.
        time.sleep_ms(750)
        return self._ds.read_temp(self._roms[0])


class BLETemperatureSensor:
    """Advertises water temperature over BLE Environmental Sensing service."""

    def __init__(self, ble, name="WaterTemp"):
        self._ble = ble
        self._ble.active(True)
        self._ble.irq(self._irq)
        ((self._handle,),) = self._ble.gatts_register_services((_ENV_SENSE_SERVICE,))
        self._connections = set()
        self._payload = _advertising_payload(
            name=name,
            services=[_ENV_SENSE_UUID],
            appearance=_ADV_APPEARANCE_GENERIC_THERMOMETER,
        )
        self._advertise()

    def _irq(self, event, data):
        if event == _IRQ_CENTRAL_CONNECT:
            conn_handle, _, _ = data
            self._connections.add(conn_handle)
        elif event == _IRQ_CENTRAL_DISCONNECT:
            conn_handle, _, _ = data
            self._connections.discard(conn_handle)
            self._advertise()

    def set_temperature(self, temp_deg_c, notify=True):
        # BLE Temperature characteristic is a signed 16-bit integer in units
        # of 0.01 degree Celsius, little-endian.
        value = int(temp_deg_c * 100)
        self._ble.gatts_write(self._handle, _encode_sint16(value))
        if notify:
            for conn_handle in self._connections:
                self._ble.gatts_notify(conn_handle, self._handle)

    def _advertise(self, interval_us=500000):
        self._ble.gap_advertise(interval_us, adv_data=self._payload)


def _encode_sint16(value):
    if value < 0:
        value += 1 << 16
    return value.to_bytes(2, "little")


def main():
    sensor = WaterTemperatureSensor()
    ble = bluetooth.BLE()
    temp_service = BLETemperatureSensor(ble)

    while True:
        try:
            temp_c = sensor.read_celsius()
            print("water temperature: {:.2f} C".format(temp_c))
            temp_service.set_temperature(temp_c)
        except onewire.OneWireError:
            print("DS18B20 read error, retrying...")
        except Exception as exc:  # noqa: BLE001 - keep the sensor loop alive
            print("unexpected error ({}), retrying: {}".format(type(exc).__name__, exc))
        time.sleep_ms(_MEASURE_INTERVAL_MS)


if __name__ == "__main__":
    main()
