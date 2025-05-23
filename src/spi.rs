use crate::gpio::*;
use crate::rcc::*;
use crate::stm32::SPI1;
use crate::time::Hertz;
use core::ptr;
pub use hal::spi::{Mode, Phase, Polarity, MODE_0, MODE_1, MODE_2, MODE_3};

/// SPI error
#[derive(Debug)]
pub enum Error {
    /// Overrun occurred
    Overrun,
    /// Mode fault occurred
    ModeFault,
    /// CRC error
    Crc,
}

/// A filler type for when the SCK pin is unnecessary
pub struct NoSck;
/// A filler type for when the Miso pin is unnecessary
pub struct NoMiso;
/// A filler type for when the Mosi pin is unnecessary
pub struct NoMosi;

pub trait Pins<SPI> {
    fn setup(&self);
    fn release(self) -> Self;
}

pub trait PinSck<SPI> {
    fn setup(&self);
    fn release(self) -> Self;
}

pub trait PinMiso<SPI> {
    fn setup(&self);
    fn release(self) -> Self;
}

pub trait PinMosi<SPI> {
    fn setup(&self);
    fn release(self) -> Self;
}

impl<SPI, SCK, MISO, MOSI> Pins<SPI> for (SCK, MISO, MOSI)
where
    SCK: PinSck<SPI>,
    MISO: PinMiso<SPI>,
    MOSI: PinMosi<SPI>,
{
    fn setup(&self) {
        self.0.setup();
        self.1.setup();
        self.2.setup();
    }

    fn release(self) -> Self {
        (self.0.release(), self.1.release(), self.2.release())
    }
}

#[derive(Debug)]
pub struct Spi<SPI, PINS> {
    spi: SPI,
    pins: PINS,
}

pub trait SpiExt: Sized {
    fn spi<PINS>(self, pins: PINS, mode: Mode, freq: Hertz, rcc: &mut Rcc) -> Spi<Self, PINS>
    where
        PINS: Pins<Self>;
}

macro_rules! spi {
    ($SPIX:ident, $spiX:ident,
        sck: [ $(($SCK:ty, $SCK_AF:expr),)+ ],
        miso: [ $(($MISO:ty, $MISO_AF:expr),)+ ],
        mosi: [ $(($MOSI:ty, $MOSI_AF:expr),)+ ],
    ) => {
        impl PinSck<$SPIX> for NoSck {
            fn setup(&self) {}

            fn release(self) -> Self {
                self
            }
        }

        impl PinMiso<$SPIX> for NoMiso {
            fn setup(&self) {}

            fn release(self) -> Self {
                self
            }
        }

        impl PinMosi<$SPIX> for NoMosi {
            fn setup(&self) {}

            fn release(self) -> Self {
                self
            }
        }

        $(
            impl PinSck<$SPIX> for $SCK {
                fn setup(&self) {
                    self.set_alt_mode($SCK_AF);
                }

                fn release(self) -> Self {
                    self.into_analog()
                }
            }
        )*
        $(
            impl PinMiso<$SPIX> for $MISO {
                fn setup(&self) {
                    self.set_alt_mode($MISO_AF);
                }

                fn release(self) -> Self {
                    self.into_analog()
                }
            }
        )*
        $(
            impl PinMosi<$SPIX> for $MOSI {
                fn setup(&self) {
                    self.set_alt_mode($MOSI_AF);
                }

                fn release(self) -> Self {
                    self.into_analog()
                }
            }
        )*

        impl<PINS: Pins<$SPIX>> Spi<$SPIX, PINS> {
            pub fn $spiX(
                spi: $SPIX,
                pins: PINS,
                mode: Mode,
                speed: Hertz,
                rcc: &mut Rcc
            ) -> Self {
                $SPIX::enable(rcc);
                $SPIX::reset(rcc);

                // disable SS output
                spi.cr2().write(|w| w.ssoe().clear_bit());

                let br = match rcc.clocks.apb_clk / speed {
                    0 => unreachable!(),
                    1..=2 => 0b000,
                    3..=5 => 0b001,
                    6..=11 => 0b010,
                    12..=23 => 0b011,
                    24..=47 => 0b100,
                    48..=95 => 0b101,
                    96..=191 => 0b110,
                    _ => 0b111,
                };

                spi.cr2().write(|w| unsafe {
                    w.frxth().set_bit().ds().bits(0b111).ssoe().clear_bit()
                });

                // Enable pins
                pins.setup();

                spi.cr1().write(|w| unsafe {
                    w.cpha()
                        .bit(mode.phase == Phase::CaptureOnSecondTransition)
                        .cpol()
                        .bit(mode.polarity == Polarity::IdleHigh)
                        .mstr()
                        .set_bit()
                        .br()
                        .bits(br)
                        .lsbfirst()
                        .clear_bit()
                        .ssm()
                        .set_bit()
                        .ssi()
                        .set_bit()
                        .rxonly()
                        .clear_bit()
                        .bidimode()
                        .clear_bit()
                        .ssi()
                        .set_bit()
                        .spe()
                        .set_bit()
                });

                Spi { spi, pins }
            }

            pub fn data_size(&mut self, nr_bits: u8) {
                self.spi.cr2().modify(|_, w| unsafe {
                    w.ds().bits(nr_bits-1)
                });
            }

            pub fn half_duplex_enable(&mut self, enable: bool) {
                self.spi.cr1().modify(|_, w|
                    w.bidimode().bit(enable)
                );
            }

            pub fn half_duplex_output_enable(&mut self, enable: bool) {
                self.spi.cr1().modify(|_, w|
                    w.bidioe().bit(enable)
                );
            }

            pub fn release(self) -> ($SPIX, PINS) {
                (self.spi, self.pins.release())
            }
        }

        impl SpiExt for $SPIX {
            fn spi<PINS>(self, pins: PINS, mode: Mode, freq: Hertz, rcc: &mut Rcc) -> Spi<$SPIX, PINS>
            where
                PINS: Pins<$SPIX>,
            {
                Spi::$spiX(self, pins, mode, freq, rcc)
            }
        }

        impl<PINS> hal::spi::FullDuplex<u8> for Spi<$SPIX, PINS> {
            type Error = Error;

            fn read(&mut self) -> nb::Result<u8, Error> {
                let sr = self.spi.sr().read();

                Err(if sr.ovr().bit_is_set() {
                    nb::Error::Other(Error::Overrun)
                } else if sr.modf().bit_is_set() {
                    nb::Error::Other(Error::ModeFault)
                } else if sr.crcerr().bit_is_set() {
                    nb::Error::Other(Error::Crc)
                } else if sr.rxne().bit_is_set() {
                    // NOTE(read_volatile) read only 1 byte (the svd2rust API only allows
                    // reading a half-word)
                    return Ok(unsafe {
                        ptr::read_volatile(&self.spi.dr() as *const _ as *const u8)
                    });
                } else {
                    nb::Error::WouldBlock
                })
            }

            fn send(&mut self, byte: u8) -> nb::Result<(), Error> {
                let sr = self.spi.sr().read();

                Err(if sr.ovr().bit_is_set() {
                    nb::Error::Other(Error::Overrun)
                } else if sr.modf().bit_is_set() {
                    nb::Error::Other(Error::ModeFault)
                } else if sr.crcerr().bit_is_set() {
                    nb::Error::Other(Error::Crc)
                } else if sr.txe().bit_is_set() {
                    unsafe {
                        self.spi.dr().write(|w| w.bits(byte as _));
                    }
                    return Ok(());
                } else {
                    nb::Error::WouldBlock
                })
            }
        }

        impl<PINS> ::hal::blocking::spi::transfer::Default<u8> for Spi<$SPIX, PINS> {}

        impl<PINS> ::hal::blocking::spi::write::Default<u8> for Spi<$SPIX, PINS> {}
    }
}

spi!(
    SPI1,
    spi1,
    sck: [
        (PA1<DefaultMode>, AltFunction::AF0),
        (PA5<DefaultMode>, AltFunction::AF0),
        (PB3<DefaultMode>, AltFunction::AF0),
        (PB6<DefaultMode>, AltFunction::AF10),
    ],
    miso: [
        (PA6<DefaultMode>, AltFunction::AF0),
        (PA11<DefaultMode>, AltFunction::AF0),
        (PB4<DefaultMode>, AltFunction::AF0),
        (PB6<DefaultMode>, AltFunction::AF9),
    ],
    mosi: [
        (PA2<DefaultMode>, AltFunction::AF0),
        (PA7<DefaultMode>, AltFunction::AF0),
        (PA12<DefaultMode>, AltFunction::AF0),
        (PB5<DefaultMode>, AltFunction::AF0),
        (PB6<DefaultMode>, AltFunction::AF8),
    ],
);
