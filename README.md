
# Hrv-rs
[![Pipeline Status](https://github.com/mat-kie/hrv-rs/actions/workflows/rust.yml/badge.svg)](https://github.com/mat-kie/hrv-rs/actions/workflows/rust.yml)
[![Coverage](https://codecov.io/gh/mat-kie/hrv-rs/branch/main/graph/badge.svg?token=YOUR_CODECOV_TOKEN)](https://codecov.io/gh/mat-kie/hrv-rs)


**Hrv-rs** is a Rust-based application designed to analyze Heart Rate Variability (HRV) using Bluetooth Low Energy (BLE) chest straps. 

## Disclaimer

**This project is in a very early stage and is not intended for any medical applications.**

## Features
- **Bluetooth Connectivity**:
  - Scan and connect to BLE chest straps that provide R-R interval data.
- **HRV Analysis**:
  - Compute HRV metrics such as RMSSD, SDRR, SD1, SD2, and Poincaré plots.
  - Visualize HRV statistics in real-time.

## HRV Metrics
- **RMSSD**: Root Mean Square of Successive Differences between R-R intervals.
- **SDRR**: Standard Deviation of R-R intervals.
- **SD1/SD2**: Short- and long-term HRV metrics derived from Poincaré plots.
- **Poincaré Plot**: A scatter plot of successive R-R intervals.

## Getting Started

### Prerequisites
- A BLE-compatible chest strap for HRV measurement.
- A system with BLE support.

### Installation
1. Clone the repository:
   ```bash
   git clone https://github.com/mat-kie/hrv-rs.git
   cd hrv-rs
   ```

2. Build the project:
   ```bash
   cargo build --release
   ```

3. Run the application:
   ```bash
   cargo run --release
   ```

## Code Structure

### Architecture
The project uses a modular, event-driven MVC architecture.

### Modules

#### 1. **Core**
- **Events**: Defines application events, including user interactions and system updates.
- **Macros**: Provides utility macros to simplify error handling and data extraction.

#### 2. **Controller**
- **Acquisition**: Handles data acquisition from BLE devices.
- **Bluetooth**: Manages Bluetooth adapters, device discovery, and communication.
- **Application**: Orchestrates application logic, including transitions between views.

#### 3. **Model**
- **Bluetooth**: Represents BLE devices, adapters, and connections.
- **HRV**: Structures and methods for storing, calculating, and retrieving HRV statistics.
- **Acquisition**: Handles runtime and stored data related to HRV measurement sessions.

#### 4. **View**
- **Bluetooth**: UI for managing Bluetooth connections.
- **HRV Analysis**: Displays computed HRV statistics and visualizes Poincaré plots.
- **Manager**: Coordinates transitions between views and manages their lifecycle.

## License
This project is licensed under the GNU General Public License. See the [LICENSE](LICENSE) file for details.

## Acknowledgments
This project uses the following libraries. Please refer to the `Cargo.toml` file for an up-to-date overview:

- [Rust Language](https://www.rust-lang.org/): The programming language that powers this project.
- [egui](https://github.com/emilk/egui): A simple, immediate-mode GUI library for Rust.
- [egui_extras](https://github.com/emilk/egui): Extensions for `egui` for richer GUI elements.
- [egui_plot](https://github.com/emilk/egui): A plotting library built for `egui`.
- [eframe](https://github.com/emilk/egui): An easy-to-use framework for building GUI applications in Rust using `egui`.
- [image](https://github.com/image-rs/image): A library for image processing, used for handling PNGs and more.
- [rfd](https://github.com/PolyMeilex/rfd): A cross-platform file and folder dialog library.
- [env_logger](https://github.com/env-logger-rs/env_logger): A flexible and human-readable logging library.
- [btleplug](https://github.com/deviceplug/btleplug): A Bluetooth Low Energy library for interacting with BLE devices.
- [uuid](https://github.com/uuid-rs/uuid): A library for generating and handling UUIDs.
- [tokio](https://github.com/tokio-rs/tokio): An asynchronous runtime for the Rust programming language.
- [futures](https://github.com/rust-lang/futures-rs): Asynchronous programming utilities for Rust.
- [nalgebra](https://nalgebra.org/): A linear algebra library for efficient mathematical computations.
- [time](https://github.com/time-rs/time): A library for date and time handling, with serialization support.
- [log](https://github.com/rust-lang/log): A lightweight logging facade for Rust.
- [serde](https://serde.rs/): A framework for serializing and deserializing Rust data structures.
- [serde_json](https://github.com/serde-rs/json): A library for working with JSON in Rust.
- [mockall](https://github.com/asomers/mockall): A mocking library for testing.