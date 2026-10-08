#include <ql/exercise.hpp>
#include <ql/instruments/vanillaoption.hpp>
#include <ql/processes/blackscholesprocess.hpp>
#include <ql/pricingengines/vanilla/fdblackscholesvanillaengine.hpp>
#include <ql/quotes/simplequote.hpp>
#include <ql/settings.hpp>
#include <ql/termstructures/volatility/equityfx/blackconstantvol.hpp>
#include <ql/termstructures/yield/flatforward.hpp>
#include <ql/time/calendars/nullcalendar.hpp>
#include <ql/time/daycounters/actual365fixed.hpp>
#include <ql/version.hpp>
#include <boost/date_time/posix_time/posix_time.hpp>
#include <cmath>
#include <cstdint>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>

using namespace QuantLib;

struct Case {
    std::string id;
    Option::Type type;
    Real spot;
    Real strike;
    Rate rate;
    Rate continuousYield;
    Volatility volatility;
    std::int64_t maturityMillis;
    Real recordedPrice;
};

struct Probe {
    const char* axis;
    Real fraction;
    const char* unit;
};

const std::vector<Probe> probes = {
    {"spot", 0.0001, "underlying-unit"},
    {"spot", 0.001, "underlying-unit"},
    {"spot", 0.01, "underlying-unit"},
    {"time", 0.001, "millisecond"},
    {"time", 0.01, "millisecond"},
    {"time", 0.05, "millisecond"},
};

std::vector<std::string> splitCsv(const std::string& line) {
    std::vector<std::string> fields;
    std::stringstream input(line);
    std::string field;
    while (std::getline(input, field, ',')) {
        fields.push_back(field);
    }
    return fields;
}

std::vector<Case> readCases(const char* path) {
    std::ifstream input(path);
    if (!input) {
        throw std::runtime_error("cannot open the pinned QuantLib grid fixture");
    }
    std::string line;
    if (!std::getline(input, line)
        || line != "kind,id,quantlib,option,t_ms,t_act365f,spot,strike,rate,continuous_yield,sigma,cash_dividend,cash_dividend_offset_ms,t_grid,x_grid,price,delta,gamma,theta") {
        throw std::runtime_error("unexpected pinned QuantLib grid fixture header");
    }

    std::vector<Case> cases;
    while (std::getline(input, line)) {
        const auto fields = splitCsv(line);
        if (fields.size() != 19 || fields[0] != "price" || fields[2] != "1.43") {
            throw std::runtime_error("unexpected row in pinned QuantLib grid fixture");
        }
        if (std::stoll(fields[13]) != 1600 || std::stoll(fields[14]) != 3200) {
            continue;
        }
        if (std::stod(fields[11]) != 0.0) {
            continue;
        }
        const Option::Type type = fields[3] == "call"
            ? Option::Call
            : fields[3] == "put" ? Option::Put : throw std::runtime_error("unknown option type");
        cases.push_back({
            fields[1], type, std::stod(fields[6]), std::stod(fields[7]),
            std::stod(fields[8]), std::stod(fields[9]), std::stod(fields[10]),
            std::stoll(fields[4]), std::stod(fields[15])
        });
    }
    if (cases.size() != 34) {
        throw std::runtime_error("expected 34 finest-grid no-cash-dividend cases");
    }
    return cases;
}

Date addMillis(const Date& base, std::int64_t millis) {
    return Date(base.dateTime() + boost::posix_time::milliseconds(millis));
}

Real priceAt(
    const Case& c,
    const Date& today,
    Real spotValue,
    std::int64_t maturityMillis
) {
    const DayCounter dayCounter = Actual365Fixed();
    const Date expiry = addMillis(today, maturityMillis);
    Settings::instance().evaluationDate() = today;
    const auto spot = ext::make_shared<SimpleQuote>(spotValue);
    const Handle<Quote> spotHandle(spot);
    const Handle<YieldTermStructure> dividendCurve(
        ext::make_shared<FlatForward>(today, c.continuousYield, dayCounter));
    const Handle<YieldTermStructure> riskFreeCurve(
        ext::make_shared<FlatForward>(today, c.rate, dayCounter));
    const Handle<BlackVolTermStructure> volatilityCurve(
        ext::make_shared<BlackConstantVol>(today, NullCalendar(), c.volatility, dayCounter));
    const auto process = ext::make_shared<BlackScholesMertonProcess>(
        spotHandle, dividendCurve, riskFreeCurve, volatilityCurve);
    const auto payoff = ext::make_shared<PlainVanillaPayoff>(c.type, c.strike);
    const auto exercise = ext::make_shared<AmericanExercise>(today, expiry);
    const auto engine = ext::make_shared<FdBlackScholesVanillaEngine>(
        process, DividendSchedule(), 1600, 3200, 2);
    VanillaOption option(payoff, exercise);
    option.setPricingEngine(engine);
    return option.NPV();
}

void emitProbe(const Case& c, const Date& today, Real basePrice, const Probe& probe) {
    Real bump = 0.0;
    Real positivePrice = 0.0;
    Real negativePrice = 0.0;
    if (std::string(probe.axis) == "spot") {
        bump = c.spot * probe.fraction;
        positivePrice = priceAt(c, today, c.spot + bump, c.maturityMillis);
        negativePrice = priceAt(c, today, c.spot - bump, c.maturityMillis);
    } else {
        bump = static_cast<Real>(std::llround(c.maturityMillis * probe.fraction));
        if (bump <= 0.0 || bump >= c.maturityMillis) {
            throw std::runtime_error("time probe bump is outside the positive expiry range");
        }
        const auto bumpMillis = static_cast<std::int64_t>(bump);
        positivePrice = priceAt(c, today, c.spot, c.maturityMillis + bumpMillis);
        negativePrice = priceAt(c, today, c.spot, c.maturityMillis - bumpMillis);
    }
    std::cout << std::setprecision(17)
              << "{\"kind\":\"price-probe\",\"id\":\"" << c.id
              << "\",\"quantlib\":\"" << QL_VERSION
              << "\",\"option\":\"" << (c.type == Option::Call ? "call" : "put")
              << "\",\"t_ms\":" << c.maturityMillis
              << ",\"t_act365f\":"
              << Actual365Fixed().yearFraction(today, addMillis(today, c.maturityMillis))
              << ",\"spot\":" << c.spot << ",\"strike\":" << c.strike
              << ",\"rate\":" << c.rate << ",\"continuous_yield\":"
              << c.continuousYield << ",\"sigma\":" << c.volatility
              << ",\"t_grid\":1600,\"x_grid\":3200,\"axis\":\"" << probe.axis
              << "\",\"bump_fraction\":" << probe.fraction
              << ",\"bump_value\":" << bump << ",\"bump_unit\":\"" << probe.unit
              << "\",\"base_price\":" << basePrice
              << ",\"positive_axis_price\":" << positivePrice
              << ",\"negative_axis_price\":" << negativePrice << "}\n";
}

int main(int argc, char** argv) {
#ifndef QL_HIGH_RESOLUTION_DATE
    std::cerr << "QL_HIGH_RESOLUTION_DATE is required for intraday expiry cases\n";
    return 2;
#endif
    if (argc != 2) {
        std::cerr << "usage: american_quantlib_v143_fd GRID.csv\n";
        return 2;
    }
    try {
        const Date today(15, June, 2026, 13, 30, 0);
        const auto cases = readCases(argv[1]);
        for (const Case& c : cases) {
            const Real basePrice = priceAt(c, today, c.spot, c.maturityMillis);
            if (std::abs(basePrice - c.recordedPrice) > 1.0e-10) {
                throw std::runtime_error("base price does not match the pinned finest-grid row: " + c.id);
            }
            for (const Probe& probe : probes) {
                emitProbe(c, today, basePrice, probe);
            }
        }
    } catch (const std::exception& error) {
        std::cerr << "QuantLib finite-difference probe failed: " << error.what() << '\n';
        return 1;
    }
    return 0;
}
