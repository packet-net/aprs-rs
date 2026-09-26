//! Every symbol the APRS symbol tables define, by name. Generated from the names Packet.Aprs uses
//! (`AprsSymbol.Named.cs`), so the names match across the implementations.

use crate::Symbol;

impl Symbol {
    /// Police, Sheriff: `/!`.
    pub const POLICE_SHERIFF: Symbol = Symbol { table: '/', code: '!' };
    /// Digi (green star with white center): `/#`.
    pub const DIGIPEATER: Symbol = Symbol { table: '/', code: '#' };
    /// Phone: `/$`.
    pub const PHONE: Symbol = Symbol { table: '/', code: '$' };
    /// DX Cluster: `/%`.
    pub const DX_CLUSTER: Symbol = Symbol { table: '/', code: '%' };
    /// HF Gateway: `/&`.
    pub const HF_GATEWAY: Symbol = Symbol { table: '/', code: '&' };
    /// Small Aircraft: `/'`.
    pub const SMALL_AIRCRAFT: Symbol = Symbol { table: '/', code: '\'' };
    /// Mobile Satellite Ground Station: `/(`.
    pub const MOBILE_SATELLITE_GROUND_STATION: Symbol = Symbol { table: '/', code: '(' };
    /// Wheelchair (handicapped): `/)`.
    pub const WHEELCHAIR: Symbol = Symbol { table: '/', code: ')' };
    /// Snowmobile: `/*`.
    pub const SNOWMOBILE: Symbol = Symbol { table: '/', code: '*' };
    /// Red Cross: `/+`.
    pub const RED_CROSS: Symbol = Symbol { table: '/', code: '+' };
    /// Boy Scouts: `/,`.
    pub const BOY_SCOUTS: Symbol = Symbol { table: '/', code: ',' };
    /// House QTH (VHF): `/-`.
    pub const HOUSE: Symbol = Symbol { table: '/', code: '-' };
    /// X: `/.`.
    pub const X_MARK: Symbol = Symbol { table: '/', code: '.' };
    /// Red Dot: `//`.
    pub const RED_DOT: Symbol = Symbol { table: '/', code: '/' };
    /// 0 Circle: `/0`.
    pub const CIRCLE_0: Symbol = Symbol { table: '/', code: '0' };
    /// 1 Circle: `/1`.
    pub const CIRCLE_1: Symbol = Symbol { table: '/', code: '1' };
    /// 2 Circle: `/2`.
    pub const CIRCLE_2: Symbol = Symbol { table: '/', code: '2' };
    /// 3 Circle: `/3`.
    pub const CIRCLE_3: Symbol = Symbol { table: '/', code: '3' };
    /// 4 Circle: `/4`.
    pub const CIRCLE_4: Symbol = Symbol { table: '/', code: '4' };
    /// 5 Circle: `/5`.
    pub const CIRCLE_5: Symbol = Symbol { table: '/', code: '5' };
    /// 6 Circle: `/6`.
    pub const CIRCLE_6: Symbol = Symbol { table: '/', code: '6' };
    /// 7 Circle: `/7`.
    pub const CIRCLE_7: Symbol = Symbol { table: '/', code: '7' };
    /// 8 Circle: `/8`.
    pub const CIRCLE_8: Symbol = Symbol { table: '/', code: '8' };
    /// 9 Circle: `/9`.
    pub const CIRCLE_9: Symbol = Symbol { table: '/', code: '9' };
    /// Fire: `/:`.
    pub const FIRE: Symbol = Symbol { table: '/', code: ':' };
    /// Campground (Portable ops): `/;`.
    pub const CAMPGROUND: Symbol = Symbol { table: '/', code: ';' };
    /// Motorcycle: `/<`.
    pub const MOTORCYCLE: Symbol = Symbol { table: '/', code: '<' };
    /// Railroad Engine: `/=`.
    pub const RAILROAD_ENGINE: Symbol = Symbol { table: '/', code: '=' };
    /// Car: `/>`.
    pub const CAR: Symbol = Symbol { table: '/', code: '>' };
    /// File Server: `/?`.
    pub const FILE_SERVER: Symbol = Symbol { table: '/', code: '?' };
    /// Hurricane Future Prediction: `/@`.
    pub const HURRICANE_FUTURE_PREDICTION: Symbol = Symbol { table: '/', code: '@' };
    /// Aid Station: `/A`.
    pub const AID_STATION: Symbol = Symbol { table: '/', code: 'A' };
    /// BBS or PBBS: `/B`.
    pub const BBS: Symbol = Symbol { table: '/', code: 'B' };
    /// Canoe: `/C`.
    pub const CANOE: Symbol = Symbol { table: '/', code: 'C' };
    /// Eyeball (events, etc.): `/E`.
    pub const EYEBALL: Symbol = Symbol { table: '/', code: 'E' };
    /// Farm Vehicle (Tractor): `/F`.
    pub const FARM_VEHICLE: Symbol = Symbol { table: '/', code: 'F' };
    /// Grid Square (6-character): `/G`.
    pub const GRID_SQUARE: Symbol = Symbol { table: '/', code: 'G' };
    /// Hotel (blue bed icon): `/H`.
    pub const HOTEL: Symbol = Symbol { table: '/', code: 'H' };
    /// TCP/IP on air network station: `/I`.
    pub const TCP_IP_NETWORK_STATION: Symbol = Symbol { table: '/', code: 'I' };
    /// School: `/K`.
    pub const SCHOOL: Symbol = Symbol { table: '/', code: 'K' };
    /// PC user: `/L`.
    pub const PC_USER: Symbol = Symbol { table: '/', code: 'L' };
    /// MacAPRS: `/M`.
    pub const MAC_APRS: Symbol = Symbol { table: '/', code: 'M' };
    /// NTS Station: `/N`.
    pub const NTS_STATION: Symbol = Symbol { table: '/', code: 'N' };
    /// Balloon: `/O`.
    pub const BALLOON: Symbol = Symbol { table: '/', code: 'O' };
    /// Police: `/P`.
    pub const POLICE: Symbol = Symbol { table: '/', code: 'P' };
    /// Recreational Vehicle: `/R`.
    pub const RECREATIONAL_VEHICLE: Symbol = Symbol { table: '/', code: 'R' };
    /// Space Shuttle: `/S`.
    pub const SPACE_SHUTTLE: Symbol = Symbol { table: '/', code: 'S' };
    /// SSTV: `/T`.
    pub const SSTV: Symbol = Symbol { table: '/', code: 'T' };
    /// Bus: `/U`.
    pub const BUS: Symbol = Symbol { table: '/', code: 'U' };
    /// Amateur TV: `/V`.
    pub const AMATEUR_TV: Symbol = Symbol { table: '/', code: 'V' };
    /// National Weather Service Site: `/W`.
    pub const NATIONAL_WEATHER_SERVICE_SITE: Symbol = Symbol { table: '/', code: 'W' };
    /// Helicopter: `/X`.
    pub const HELICOPTER: Symbol = Symbol { table: '/', code: 'X' };
    /// Yacht (sail boat): `/Y`.
    pub const YACHT: Symbol = Symbol { table: '/', code: 'Y' };
    /// WinAPRS: `/Z`.
    pub const WIN_APRS: Symbol = Symbol { table: '/', code: 'Z' };
    /// Jogger, Human/person: `/[`.
    pub const JOGGER: Symbol = Symbol { table: '/', code: '[' };
    /// Triangle (DF): `/\`.
    pub const DIRECTION_FINDING: Symbol = Symbol { table: '/', code: '\\' };
    /// Mail/Post Office: `/]`.
    pub const POST_OFFICE: Symbol = Symbol { table: '/', code: ']' };
    /// Large Aircraft: `/^`.
    pub const LARGE_AIRCRAFT: Symbol = Symbol { table: '/', code: '^' };
    /// Weather Station (blue): `/_`.
    pub const WEATHER_STATION: Symbol = Symbol { table: '/', code: '_' };
    /// Dish Antenna: `` /` ``.
    pub const DISH_ANTENNA: Symbol = Symbol { table: '/', code: '`' };
    /// Ambulance: `/a`.
    pub const AMBULANCE: Symbol = Symbol { table: '/', code: 'a' };
    /// Bicycle: `/b`.
    pub const BICYCLE: Symbol = Symbol { table: '/', code: 'b' };
    /// Incident Command Post: `/c`.
    pub const INCIDENT_COMMAND_POST: Symbol = Symbol { table: '/', code: 'c' };
    /// Fire Department: `/d`.
    pub const FIRE_DEPARTMENT: Symbol = Symbol { table: '/', code: 'd' };
    /// Horse (equestrian): `/e`.
    pub const HORSE: Symbol = Symbol { table: '/', code: 'e' };
    /// Fire Truck: `/f`.
    pub const FIRE_TRUCK: Symbol = Symbol { table: '/', code: 'f' };
    /// Glider: `/g`.
    pub const GLIDER: Symbol = Symbol { table: '/', code: 'g' };
    /// Hospital: `/h`.
    pub const HOSPITAL: Symbol = Symbol { table: '/', code: 'h' };
    /// IOTA (Islands on the Air): `/i`.
    pub const IOTA: Symbol = Symbol { table: '/', code: 'i' };
    /// Jeep: `/j`.
    pub const JEEP: Symbol = Symbol { table: '/', code: 'j' };
    /// Truck: `/k`.
    pub const TRUCK: Symbol = Symbol { table: '/', code: 'k' };
    /// Laptop: `/l`.
    pub const LAPTOP: Symbol = Symbol { table: '/', code: 'l' };
    /// Mic-E Repeater: `/m`.
    pub const MIC_E_REPEATER: Symbol = Symbol { table: '/', code: 'm' };
    /// Node (black bulls-eye): `/n`.
    pub const NODE: Symbol = Symbol { table: '/', code: 'n' };
    /// Emergency Operations Center: `/o`.
    pub const EMERGENCY_OPERATIONS_CENTER: Symbol = Symbol { table: '/', code: 'o' };
    /// Rover (puppy dog): `/p`.
    pub const ROVER: Symbol = Symbol { table: '/', code: 'p' };
    /// Grid Square shown above 128m: `/q`.
    pub const GRID_SQUARE_ABOVE_128M: Symbol = Symbol { table: '/', code: 'q' };
    /// Repeater: `/r`.
    pub const REPEATER: Symbol = Symbol { table: '/', code: 'r' };
    /// Ship (power boat): `/s`.
    pub const SHIP: Symbol = Symbol { table: '/', code: 's' };
    /// Truck Stop: `/t`.
    pub const TRUCK_STOP: Symbol = Symbol { table: '/', code: 't' };
    /// Truck (18-wheeler): `/u`.
    pub const EIGHTEEN_WHEELER: Symbol = Symbol { table: '/', code: 'u' };
    /// Van: `/v`.
    pub const VAN: Symbol = Symbol { table: '/', code: 'v' };
    /// Water Station: `/w`.
    pub const WATER_STATION: Symbol = Symbol { table: '/', code: 'w' };
    /// X-APRS (Unix): `/x`.
    pub const X_APRS: Symbol = Symbol { table: '/', code: 'x' };
    /// Yagi at QTH: `/y`.
    pub const YAGI_AT_QTH: Symbol = Symbol { table: '/', code: 'y' };
    /// Emergency: `\!`.
    pub const EMERGENCY: Symbol = Symbol { table: '\\', code: '!' };
    /// Digi (green star): `\#`.
    pub const OVERLAY_DIGIPEATER: Symbol = Symbol { table: '\\', code: '#' };
    /// Bank or ATM (green box): `\$`.
    pub const BANK: Symbol = Symbol { table: '\\', code: '$' };
    /// Power Plant: `\%`.
    pub const POWER_PLANT: Symbol = Symbol { table: '\\', code: '%' };
    /// I=IGate R=RX T=1hopTX 2=2hopTX: `\&`.
    pub const GATEWAY: Symbol = Symbol { table: '\\', code: '&' };
    /// Crash (& incident sites): `\'`.
    pub const CRASH: Symbol = Symbol { table: '\\', code: '\'' };
    /// Cloudy: `\(`.
    pub const CLOUDY: Symbol = Symbol { table: '\\', code: '(' };
    /// Firenet MEO, MODIS Earth Obs.: `\)`.
    pub const FIRENET: Symbol = Symbol { table: '\\', code: ')' };
    /// Snow: `\*`.
    pub const SNOW: Symbol = Symbol { table: '\\', code: '*' };
    /// Church: `\+`.
    pub const CHURCH: Symbol = Symbol { table: '\\', code: '+' };
    /// Girl Scouts: `\,`.
    pub const GIRL_SCOUTS: Symbol = Symbol { table: '\\', code: ',' };
    /// House (H=HF) (O = Op Present): `\-`.
    pub const OVERLAY_HOUSE: Symbol = Symbol { table: '\\', code: '-' };
    /// Ambiguous (Big Question Mark): `\.`.
    pub const AMBIGUOUS: Symbol = Symbol { table: '\\', code: '.' };
    /// Waypoint Destination (Note 1): `\/`.
    pub const WAYPOINT: Symbol = Symbol { table: '\\', code: '/' };
    /// Circle (E/I/W= IRLP/EchoLink/WIRES): `\0`.
    pub const OVERLAY_CIRCLE: Symbol = Symbol { table: '\\', code: '0' };
    /// 802.11 or other network node: `\8`.
    pub const NETWORK_NODE: Symbol = Symbol { table: '\\', code: '8' };
    /// Gas Station (blue pump): `\9`.
    pub const GAS_STATION: Symbol = Symbol { table: '\\', code: '9' };
    /// Hail: `\:`.
    pub const HAIL: Symbol = Symbol { table: '\\', code: ':' };
    /// Park/Picnic Area: `\;`.
    pub const PARK: Symbol = Symbol { table: '\\', code: ';' };
    /// Advisory (one WX flag): `\<`.
    pub const ADVISORY: Symbol = Symbol { table: '\\', code: '<' };
    /// APRStt Touchtone (DTMF Users): `\=`.
    pub const APRSTT: Symbol = Symbol { table: '\\', code: '=' };
    /// Cars & Vehicles: `\>`.
    pub const OVERLAY_VEHICLE: Symbol = Symbol { table: '\\', code: '>' };
    /// Information Kiosk (blue box with ?): `\?`.
    pub const INFORMATION_KIOSK: Symbol = Symbol { table: '\\', code: '?' };
    /// Hurricane/Tropical Storm: `\@`.
    pub const HURRICANE_TROPICAL_STORM: Symbol = Symbol { table: '\\', code: '@' };
    /// Box: `\A`.
    pub const OVERLAY_BOX: Symbol = Symbol { table: '\\', code: 'A' };
    /// Blowing Snow: `\B`.
    pub const BLOWING_SNOW: Symbol = Symbol { table: '\\', code: 'B' };
    /// Coast Guard: `\C`.
    pub const COAST_GUARD: Symbol = Symbol { table: '\\', code: 'C' };
    /// Drizzle: `\D`.
    pub const DRIZZLE: Symbol = Symbol { table: '\\', code: 'D' };
    /// Smoke (& other vis codes): `\E`.
    pub const SMOKE: Symbol = Symbol { table: '\\', code: 'E' };
    /// Freezing Rain: `\F`.
    pub const FREEZING_RAIN: Symbol = Symbol { table: '\\', code: 'F' };
    /// Snow Shower: `\G`.
    pub const SNOW_SHOWER: Symbol = Symbol { table: '\\', code: 'G' };
    /// Haze: `\H`.
    pub const HAZE: Symbol = Symbol { table: '\\', code: 'H' };
    /// Rain Shower: `\I`.
    pub const RAIN_SHOWER: Symbol = Symbol { table: '\\', code: 'I' };
    /// Lightning: `\J`.
    pub const LIGHTNING: Symbol = Symbol { table: '\\', code: 'J' };
    /// Kenwood HT (w): `\K`.
    pub const KENWOOD_HT: Symbol = Symbol { table: '\\', code: 'K' };
    /// Lighthouse: `\L`.
    pub const LIGHTHOUSE: Symbol = Symbol { table: '\\', code: 'L' };
    /// MARS (A=Army, N=Navy, F=AF): `\M`.
    pub const MARS: Symbol = Symbol { table: '\\', code: 'M' };
    /// Navigation Buoy: `\N`.
    pub const NAVIGATION_BUOY: Symbol = Symbol { table: '\\', code: 'N' };
    /// Rocket: `\O`.
    pub const ROCKET: Symbol = Symbol { table: '\\', code: 'O' };
    /// Parking: `\P`.
    pub const PARKING: Symbol = Symbol { table: '\\', code: 'P' };
    /// Earthquake: `\Q`.
    pub const EARTHQUAKE: Symbol = Symbol { table: '\\', code: 'Q' };
    /// Restaurant: `\R`.
    pub const RESTAURANT: Symbol = Symbol { table: '\\', code: 'R' };
    /// Satellite: `\S`.
    pub const SATELLITE: Symbol = Symbol { table: '\\', code: 'S' };
    /// Thunderstorm: `\T`.
    pub const THUNDERSTORM: Symbol = Symbol { table: '\\', code: 'T' };
    /// Sunny: `\U`.
    pub const SUNNY: Symbol = Symbol { table: '\\', code: 'U' };
    /// VORTAC Nav Aid: `\V`.
    pub const VORTAC: Symbol = Symbol { table: '\\', code: 'V' };
    /// NWS Site: `\W`.
    pub const OVERLAY_NWS_SITE: Symbol = Symbol { table: '\\', code: 'W' };
    /// Pharmacy Rx: `\X`.
    pub const PHARMACY: Symbol = Symbol { table: '\\', code: 'X' };
    /// Radios and devices: `\Y`.
    pub const OVERLAY_RADIO: Symbol = Symbol { table: '\\', code: 'Y' };
    /// Wall Cloud: `\[`.
    pub const WALL_CLOUD: Symbol = Symbol { table: '\\', code: '[' };
    /// Aircraft (Shows Heading): `\^`.
    pub const AIRCRAFT_WITH_HEADING: Symbol = Symbol { table: '\\', code: '^' };
    /// WX Station with Digi (green): `\_`.
    pub const WEATHER_STATION_WITH_DIGIPEATER: Symbol = Symbol { table: '\\', code: '_' };
    /// Rain: `` \` ``.
    pub const RAIN: Symbol = Symbol { table: '\\', code: '`' };
    /// ARRL, ARES, WinLINK, Dstar, LoRa, etc: `\a`.
    pub const OVERLAY_DIAMOND: Symbol = Symbol { table: '\\', code: 'a' };
    /// Blowing Dust/Sand: `\b`.
    pub const BLOWING_DUST: Symbol = Symbol { table: '\\', code: 'b' };
    /// CD triangle RACES/SATERN/etc: `\c`.
    pub const OVERLAY_CIVIL_DEFENSE: Symbol = Symbol { table: '\\', code: 'c' };
    /// DX Spot (from callsign prefix): `\d`.
    pub const DX_SPOT: Symbol = Symbol { table: '\\', code: 'd' };
    /// Sleet: `\e`.
    pub const SLEET: Symbol = Symbol { table: '\\', code: 'e' };
    /// Funnel Cloud: `\f`.
    pub const FUNNEL_CLOUD: Symbol = Symbol { table: '\\', code: 'f' };
    /// Gale Flags: `\g`.
    pub const GALE_FLAGS: Symbol = Symbol { table: '\\', code: 'g' };
    /// Store or Hamfest: `\h`.
    pub const STORE: Symbol = Symbol { table: '\\', code: 'h' };
    /// BOX or points of Interest: `\i`.
    pub const POINT_OF_INTEREST: Symbol = Symbol { table: '\\', code: 'i' };
    /// Work Zone (steam shovel): `\j`.
    pub const WORK_ZONE: Symbol = Symbol { table: '\\', code: 'j' };
    /// Special Vehicle SUV, ATV, 4x4: `\k`.
    pub const SPECIAL_VEHICLE: Symbol = Symbol { table: '\\', code: 'k' };
    /// Area Symbols (box, circle, etc): `\l`.
    pub const AREA: Symbol = Symbol { table: '\\', code: 'l' };
    /// Value Sign (3 digit display): `\m`.
    pub const VALUE_SIGN: Symbol = Symbol { table: '\\', code: 'm' };
    /// Triangle: `\n`.
    pub const TRIANGLE: Symbol = Symbol { table: '\\', code: 'n' };
    /// Small Circle: `\o`.
    pub const SMALL_CIRCLE: Symbol = Symbol { table: '\\', code: 'o' };
    /// Partly Cloudy: `\p`.
    pub const PARTLY_CLOUDY: Symbol = Symbol { table: '\\', code: 'p' };
    /// Restrooms: `\r`.
    pub const RESTROOMS: Symbol = Symbol { table: '\\', code: 'r' };
    /// Ship/Boat (top view): `\s`.
    pub const OVERLAY_SHIP: Symbol = Symbol { table: '\\', code: 's' };
    /// Tornado: `\t`.
    pub const TORNADO: Symbol = Symbol { table: '\\', code: 't' };
    /// Truck: `\u`.
    pub const OVERLAY_TRUCK: Symbol = Symbol { table: '\\', code: 'u' };
    /// Van: `\v`.
    pub const OVERLAY_VAN: Symbol = Symbol { table: '\\', code: 'v' };
    /// Flooding (Avalanches/Slides): `\w`.
    pub const FLOODING: Symbol = Symbol { table: '\\', code: 'w' };
    /// Wreck or Obstruction: `\x`.
    pub const WRECK: Symbol = Symbol { table: '\\', code: 'x' };
    /// Skywarn: `\y`.
    pub const SKYWARN: Symbol = Symbol { table: '\\', code: 'y' };
    /// Overlayed Shelter: `\z`.
    pub const SHELTER: Symbol = Symbol { table: '\\', code: 'z' };
    /// Fog: `\{`.
    pub const FOG: Symbol = Symbol { table: '\\', code: '{' };
}
