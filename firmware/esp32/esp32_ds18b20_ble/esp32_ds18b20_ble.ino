/*
 * water_temperature - ESP-WROOM-32D firmware
 *
 * Reads the water temperature from a DS18B20 (1-Wire) sensor and exposes it
 * over Bluetooth Low Energy using the standard "Environmental Sensing"
 * service (0x181A) with a "Temperature" characteristic (0x2A6E), so it can be
 * read directly from a web browser (e.g. Android Chrome) using the
 * Web Bluetooth API. See docs/index.html in this repository for a ready to
 * use client. The GATT UUIDs match the Raspberry Pi Pico W firmware in
 * firmware/pico_w/main.py, so the same web client works with either board.
 *
 * Wiring (DS18B20):
 *   VDD -> 3V3
 *   GND -> GND
 *   DQ  -> GPIO4 with a 4.7k pull-up resistor between DQ and VDD
 *
 * Required libraries (install via Arduino Library Manager):
 *   - OneWire
 *   - DallasTemperature
 *   - ESP32 BLE Arduino (bundled with the ESP32 Arduino core)
 */

#include <Arduino.h>
#include <BLEDevice.h>
#include <BLEServer.h>
#include <BLEUtils.h>
#include <BLE2902.h>
#include <OneWire.h>
#include <DallasTemperature.h>

// org.bluetooth.service.environmental_sensing
#define ENV_SENSE_SERVICE_UUID BLEUUID((uint16_t)0x181A)
// org.bluetooth.characteristic.temperature
#define TEMPERATURE_CHAR_UUID BLEUUID((uint16_t)0x2A6E)

static const uint8_t ONE_WIRE_BUS = 4;
static const unsigned long MEASURE_INTERVAL_MS = 5000;

OneWire oneWire(ONE_WIRE_BUS);
DallasTemperature sensors(&oneWire);

BLEServer *bleServer = nullptr;
BLECharacteristic *temperatureChar = nullptr;
bool deviceConnected = false;
unsigned long lastMeasureMs = 0;

class ServerCallbacks : public BLEServerCallbacks {
  void onConnect(BLEServer *server) override {
    deviceConnected = true;
  }

  void onDisconnect(BLEServer *server) override {
    deviceConnected = false;
    server->getAdvertising()->start();
  }
};

// Encodes a temperature in Celsius as the BLE Temperature characteristic
// format: a little-endian signed 16-bit integer in units of 0.01 degree C.
static void encodeTemperature(float celsius, uint8_t out[2]) {
  int16_t value = (int16_t)round(celsius * 100.0f);
  out[0] = (uint8_t)(value & 0xFF);
  out[1] = (uint8_t)((value >> 8) & 0xFF);
}

void setup() {
  Serial.begin(115200);
  sensors.begin();

  BLEDevice::init("WaterTemp");
  bleServer = BLEDevice::createServer();
  bleServer->setCallbacks(new ServerCallbacks());

  BLEService *service = bleServer->createService(ENV_SENSE_SERVICE_UUID);
  temperatureChar = service->createCharacteristic(
      TEMPERATURE_CHAR_UUID,
      BLECharacteristic::PROPERTY_READ | BLECharacteristic::PROPERTY_NOTIFY);
  temperatureChar->addDescriptor(new BLE2902());
  service->start();

  BLEAdvertising *advertising = bleServer->getAdvertising();
  advertising->addServiceUUID(ENV_SENSE_SERVICE_UUID);
  advertising->setAppearance(768);  // Generic Thermometer
  advertising->start();

  Serial.println("BLE water temperature sensor ready, advertising as \"WaterTemp\"");
}

void loop() {
  unsigned long now = millis();
  if (now - lastMeasureMs >= MEASURE_INTERVAL_MS) {
    lastMeasureMs = now;

    sensors.requestTemperatures();
    // Blocks for up to ~750ms while the DS18B20 performs a 12-bit
    // conversion (DallasTemperature's default wait-for-conversion mode).
    float tempC = sensors.getTempCByIndex(0);

    if (tempC == DEVICE_DISCONNECTED_C) {
      Serial.println("DS18B20 read error, retrying...");
    } else {
      Serial.printf("water temperature: %.2f C\n", tempC);
      uint8_t payload[2];
      encodeTemperature(tempC, payload);
      temperatureChar->setValue(payload, sizeof(payload));
      if (deviceConnected) {
        temperatureChar->notify();
      }
    }
  }
}
