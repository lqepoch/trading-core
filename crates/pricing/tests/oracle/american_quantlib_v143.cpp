#include <ql/instruments/vanillaoption.hpp>
#include <ql/exercise.hpp>
#include <ql/quotes/simplequote.hpp>
#include <ql/termstructures/yield/flatforward.hpp>
#include <ql/termstructures/volatility/equityfx/blackconstantvol.hpp>
#include <ql/time/daycounters/actual365fixed.hpp>
#include <ql/time/calendars/nullcalendar.hpp>
#include <ql/cashflows/dividend.hpp>
#include <ql/pricingengines/vanilla/fdblackscholesvanillaengine.hpp>
#include <ql/processes/blackscholesprocess.hpp>
#include <ql/settings.hpp>
#include <ql/version.hpp>
#include <boost/date_time/posix_time/posix_time.hpp>
#include <cmath>
#include <cstdint>
#include <exception>
#include <iomanip>
#include <iostream>
#include <string>
#include <vector>

using namespace QuantLib;

struct Case {
    const char* id;
    Option::Type type;
    Real spot;
    Real strike;
    Rate rate;
    Rate continuousYield;
    Volatility volatility;
    std::int64_t maturityMillis;
    Real cashDividend;
    std::int64_t cashDividendOffsetMillis;
};

struct Grid { Size time; Size space; };

Date addMillis(const Date& base, std::int64_t millis) {
    return Date(base.dateTime() + boost::posix_time::milliseconds(millis));
}

void run(const Case& c, const Date& today, const Grid& grid) {
    try {
        const DayCounter dc = Actual365Fixed();
        const Date expiry = addMillis(today, c.maturityMillis);
        Settings::instance().evaluationDate() = today;

        const auto spot = ext::make_shared<SimpleQuote>(c.spot);
        const Handle<Quote> spotHandle(spot);
        const Handle<YieldTermStructure> dividendCurve(
            ext::make_shared<FlatForward>(today, c.continuousYield, dc));
        const Handle<YieldTermStructure> riskFreeCurve(
            ext::make_shared<FlatForward>(today, c.rate, dc));
        const Handle<BlackVolTermStructure> volatilityCurve(
            ext::make_shared<BlackConstantVol>(today, NullCalendar(), c.volatility, dc));
        const auto process = ext::make_shared<BlackScholesMertonProcess>(
            spotHandle, dividendCurve, riskFreeCurve, volatilityCurve);
        const auto payoff = ext::make_shared<PlainVanillaPayoff>(c.type, c.strike);
        const auto exercise = ext::make_shared<AmericanExercise>(today, expiry);
        DividendSchedule dividends;
        if (c.cashDividend > 0.0) {
            dividends.push_back(ext::make_shared<FixedDividend>(
                c.cashDividend, addMillis(today, c.cashDividendOffsetMillis)));
        }
        const auto engine = ext::make_shared<FdBlackScholesVanillaEngine>(
            process, dividends, grid.time, grid.space, 2);
        VanillaOption option(payoff, exercise);
        option.setPricingEngine(engine);
        std::cout << std::setprecision(17)
                  << "{\"kind\":\"price\",\"id\":\"" << c.id
                  << "\",\"quantlib\":\"" << QL_VERSION << "\",\"t_ms\":"
                  << c.maturityMillis << ",\"t_act365f\":"
                  << dc.yearFraction(today, expiry) << ",\"option\":\""
                  << (c.type == Option::Call ? "call" : "put")
                  << "\",\"spot\":" << c.spot << ",\"strike\":" << c.strike
                  << ",\"rate\":" << c.rate << ",\"continuous_yield\":"
                  << c.continuousYield << ",\"sigma\":" << c.volatility
                  << ",\"cash_dividend\":" << c.cashDividend
                  << ",\"cash_dividend_offset_ms\":" << c.cashDividendOffsetMillis
                  << ",\"t_grid\":" << grid.time << ",\"x_grid\":" << grid.space
                  << ",\"price\":" << option.NPV() << ",\"delta\":"
                  << option.delta() << ",\"gamma\":" << option.gamma()
                  << ",\"theta\":" << option.theta() << "}\n";
    } catch (const std::exception& e) {
        std::cout << "{\"kind\":\"error\",\"id\":\"" << c.id
                  << "\",\"t_grid\":" << grid.time << ",\"x_grid\":"
                  << grid.space << ",\"error\":\"" << e.what() << "\"}\n";
    }
}

int main() {
#ifndef QL_HIGH_RESOLUTION_DATE
    std::cerr << "QL_HIGH_RESOLUTION_DATE is required for intraday expiry cases\n";
    return 2;
#endif
    const Date today(15, June, 2026, 13, 30, 0);
    Settings::instance().evaluationDate() = today;
    const std::int64_t day = 86'400'000;
    const std::vector<Case> cases = {
        {"american_call_90d_atm_no_div", Option::Call, 100, 100, 0.04, 0.00, 0.25, 90 * day, 0, 0},
        {"american_put_180d_itm_no_div", Option::Put, 95, 100, 0.05, 0.00, 0.30, 180 * day, 0, 0},
        {"american_call_180d_itm_q04", Option::Call, 110, 100, 0.04, 0.04, 0.25, 180 * day, 0, 0},
        {"american_put_90d_otm_q02", Option::Put, 105, 100, 0.02, 0.02, 0.20, 90 * day, 0, 0},
        {"american_put_30d_negative_r", Option::Put, 90, 100, -0.02, 0.00, 0.25, 30 * day, 0, 0},
        {"0dte_atm_call_1h_no_div", Option::Call, 100, 100, 0.04, 0.00, 0.25, 3'600'000, 0, 0},
        {"0dte_atm_put_60001ms_no_div", Option::Put, 100, 100, 0.05, 0.00, 0.25, 60'001, 0, 0},
        {"0dte_atm_put_60s_no_div", Option::Put, 100, 100, 0.05, 0.00, 0.25, 60'000, 0, 0},
        {"0dte_atm_put_59999ms_no_div", Option::Put, 100, 100, 0.05, 0.00, 0.25, 59'999, 0, 0},
        {"0dte_itm_put_60s_no_div", Option::Put, 99, 100, 0.05, 0.00, 0.25, 60'000, 0, 0},
        {"0dte_itm_put_60001ms_no_div", Option::Put, 99, 100, 0.05, 0.00, 0.25, 60'001, 0, 0},
        {"0dte_itm_put_59999ms_no_div", Option::Put, 99, 100, 0.05, 0.00, 0.25, 59'999, 0, 0},
        {"american_call_30d_no_div_baseline", Option::Call, 102, 100, 0.03, 0.00, 0.25, 30 * day, 0, 0},
        {"american_call_30d_cash_div_1", Option::Call, 102, 100, 0.03, 0.00, 0.25, 30 * day, 1.0, 10 * day},
        {"american_put_30d_no_div_baseline", Option::Put, 98, 100, 0.03, 0.00, 0.25, 30 * day, 0, 0},
        {"american_put_30d_cash_div_1", Option::Put, 98, 100, 0.03, 0.00, 0.25, 30 * day, 1.0, 10 * day},
        {"matrix_call_80_60s", Option::Call, 80, 100, 0.00, 0.00, 0.20, 60'001, 0, 0},
        {"matrix_put_80_1d", Option::Put, 80, 100, -0.05, -0.02, 0.05, 1 * day, 0, 0},
        {"matrix_call_95_1h", Option::Call, 95, 100, 0.15, 0.08, 0.50, 3'600'000, 0, 0},
        {"matrix_put_95_30d", Option::Put, 95, 100, -0.15, 0.08, 0.20, 30 * day, 0, 0},
        {"matrix_call_100_1d", Option::Call, 100, 100, -0.05, 0.08, 0.50, 1 * day, 0, 0},
        {"matrix_put_100_90d", Option::Put, 100, 100, 0.15, -0.02, 1.00, 90 * day, 0, 0},
        {"matrix_call_105_30d", Option::Call, 105, 100, 0.00, 0.08, 2.00, 30 * day, 0, 0},
        {"matrix_put_105_365d", Option::Put, 105, 100, 0.00, 0.00, 0.05, 365 * day, 0, 0},
        {"matrix_call_120_30d", Option::Call, 120, 100, 0.05, 0.02, 2.00, 30 * day, 0, 0},
        {"matrix_put_120_1d", Option::Put, 120, 100, -0.05, 0.08, 2.00, 1 * day, 0, 0},
        {"matrix_call_80_365d", Option::Call, 80, 100, -0.05, -0.02, 0.05, 365 * day, 0, 0},
        {"matrix_put_80_90d", Option::Put, 80, 100, 0.15, 0.08, 0.50, 90 * day, 0, 0},
        {"matrix_call_95_1d", Option::Call, 95, 100, -0.15, 0.08, 0.20, 1 * day, 0, 0},
        {"matrix_put_95_365d", Option::Put, 95, 100, 0.15, -0.02, 1.00, 365 * day, 0, 0},
        {"matrix_call_100_60s", Option::Call, 100, 100, 0.05, 0.00, 0.25, 60'000, 0, 0},
        {"matrix_put_100_365d", Option::Put, 100, 100, -0.05, 0.08, 2.00, 365 * day, 0, 0},
        {"matrix_call_105_1h", Option::Call, 105, 100, 0.15, 0.08, 0.50, 3'600'000, 0, 0},
        {"matrix_put_105_30d", Option::Put, 105, 100, -0.15, 0.08, 0.20, 30 * day, 0, 0},
        {"matrix_call_120_90d", Option::Call, 120, 100, 0.00, 0.08, 2.00, 90 * day, 0, 0},
        {"matrix_put_120_180d", Option::Put, 120, 100, 0.05, 0.02, 2.00, 180 * day, 0, 0},
    };
    const std::vector<Grid> grids = {
        {100, 400}, {400, 800}, {800, 1600}, {1600, 3200}
    };
    for (const Case& c : cases) {
        for (const Grid& grid : grids) {
            run(c, today, grid);
        }
    }
    return 0;
}
