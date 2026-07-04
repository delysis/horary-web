#import <CoreLocation/CoreLocation.h>
#import <Foundation/Foundation.h>
#import <dispatch/dispatch.h>
#include <stdarg.h>
#include <stddef.h>
#include <stdlib.h>
#include <string.h>

typedef struct HoraryNativeLocationResult {
    double latitude;
    double longitude;
    double horizontal_accuracy;
    int authorization_status;
} HoraryNativeLocationResult;

typedef struct HoraryLocationState {
    dispatch_semaphore_t semaphore;
    HoraryNativeLocationResult result;
    char error[512];
    BOOL completed;
    BOOL requested_location;
} HoraryLocationState;

static void horary_location_set_error(HoraryLocationState *state, NSString *format, ...) {
    if (!state || state->completed) {
        return;
    }

    va_list args;
    va_start(args, format);
    NSString *message = [[NSString alloc] initWithFormat:format arguments:args];
    va_end(args);

    const char *utf8 = [message UTF8String];
    if (!utf8) {
        utf8 = "Location detection failed.";
    }
    strlcpy(state->error, utf8, sizeof(state->error));
}

static void horary_location_complete(HoraryLocationState *state) {
    if (!state || state->completed) {
        return;
    }
    state->completed = YES;
    dispatch_semaphore_signal(state->semaphore);
}

static CLAuthorizationStatus horary_location_authorization_status(CLLocationManager *manager) {
    if (@available(macOS 11.0, *)) {
        return [manager authorizationStatus];
    }
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
    return [CLLocationManager authorizationStatus];
#pragma clang diagnostic pop
}

static void horary_location_request_authorization(CLLocationManager *manager) {
    if (@available(macOS 10.15, *)) {
        [manager requestWhenInUseAuthorization];
    }
    [manager startUpdatingLocation];
}

static void horary_location_request_once(CLLocationManager *manager) {
    [manager startUpdatingLocation];
}

@interface HoraryLocationDelegate : NSObject <CLLocationManagerDelegate>
- (instancetype)initWithState:(HoraryLocationState *)state;
- (void)invalidate;
- (void)handleAuthorizationStatus:(CLAuthorizationStatus)status manager:(CLLocationManager *)manager;
@end

@implementation HoraryLocationDelegate {
    HoraryLocationState *_state;
}

- (instancetype)initWithState:(HoraryLocationState *)state {
    self = [super init];
    if (self) {
        _state = state;
    }
    return self;
}

- (void)invalidate {
    _state = NULL;
}

- (void)requestOneLocation:(CLLocationManager *)manager {
    if (_state->requested_location) {
        return;
    }
    _state->requested_location = YES;

    horary_location_request_once(manager);
}

- (void)handleAuthorizationStatus:(CLAuthorizationStatus)status manager:(CLLocationManager *)manager {
    if (!_state || _state->completed) {
        return;
    }

    _state->result.authorization_status = (int)status;

    switch (status) {
        case kCLAuthorizationStatusNotDetermined:
            horary_location_request_authorization(manager);
            break;
        case kCLAuthorizationStatusRestricted:
            horary_location_set_error(_state, @"Location Services are restricted on this Mac.");
            horary_location_complete(_state);
            break;
        case kCLAuthorizationStatusDenied:
            horary_location_set_error(_state, @"Location access is blocked for Horary in macOS Location Services.");
            horary_location_complete(_state);
            break;
        case kCLAuthorizationStatusAuthorizedAlways:
        case 4:
            [self requestOneLocation:manager];
            break;
        default:
            horary_location_set_error(_state, @"Unsupported CoreLocation authorization status: %d.", (int)status);
            horary_location_complete(_state);
            break;
    }
}

- (void)locationManagerDidChangeAuthorization:(CLLocationManager *)manager {
    [self handleAuthorizationStatus:horary_location_authorization_status(manager) manager:manager];
}

- (void)locationManager:(CLLocationManager *)manager didChangeAuthorizationStatus:(CLAuthorizationStatus)status {
    [self handleAuthorizationStatus:status manager:manager];
}

- (void)locationManager:(CLLocationManager *)manager didUpdateLocations:(NSArray<CLLocation *> *)locations {
    if (!_state || _state->completed) {
        return;
    }

    CLLocation *location = [locations lastObject];
    if (!location || !CLLocationCoordinate2DIsValid(location.coordinate)) {
        horary_location_set_error(_state, @"CoreLocation returned no valid location.");
        horary_location_complete(_state);
        return;
    }

    _state->result.latitude = location.coordinate.latitude;
    _state->result.longitude = location.coordinate.longitude;
    _state->result.horizontal_accuracy = location.horizontalAccuracy;
    _state->result.authorization_status = (int)horary_location_authorization_status(manager);
    [manager stopUpdatingLocation];
    horary_location_complete(_state);
}

- (void)locationManager:(CLLocationManager *)manager didFailWithError:(NSError *)error {
    if (!_state || _state->completed) {
        return;
    }

    if (error) {
        horary_location_set_error(_state, @"CoreLocation failed: %@", [error localizedDescription]);
    } else {
        horary_location_set_error(_state, @"CoreLocation failed.");
    }
    [manager stopUpdatingLocation];
    horary_location_complete(_state);
}

@end

int horary_request_current_location(
    double timeout_seconds,
    HoraryNativeLocationResult *out_result,
    char *error_buffer,
    size_t error_buffer_len
) {
    if (!out_result || !error_buffer || error_buffer_len == 0) {
        return 1;
    }

    memset(out_result, 0, sizeof(*out_result));
    error_buffer[0] = '\0';

    @autoreleasepool {
        HoraryLocationState *state = calloc(1, sizeof(*state));
        if (!state) {
            strlcpy(error_buffer, "Could not allocate CoreLocation state.", error_buffer_len);
            return 1;
        }
        state->semaphore = dispatch_semaphore_create(0);

        __block CLLocationManager *manager = nil;
        __block HoraryLocationDelegate *delegate = nil;

        dispatch_async(dispatch_get_main_queue(), ^{
            if (state->completed) {
                return;
            }
            if (![CLLocationManager locationServicesEnabled]) {
                horary_location_set_error(state, @"Location Services are disabled on this Mac.");
                horary_location_complete(state);
                return;
            }

            manager = [[CLLocationManager alloc] init];
            manager.desiredAccuracy = kCLLocationAccuracyKilometer;
            manager.distanceFilter = kCLDistanceFilterNone;

            delegate = [[HoraryLocationDelegate alloc] initWithState:state];
            manager.delegate = delegate;
            [delegate handleAuthorizationStatus:horary_location_authorization_status(manager) manager:manager];
        });

        uint64_t timeout_nanos = (uint64_t)(timeout_seconds * (double)NSEC_PER_SEC);
        if (timeout_nanos == 0) {
            timeout_nanos = 6 * NSEC_PER_SEC;
        }
        long wait_result = dispatch_semaphore_wait(
            state->semaphore,
            dispatch_time(DISPATCH_TIME_NOW, (int64_t)timeout_nanos)
        );

        void (^cleanup)(void) = ^{
            if (delegate) {
                [delegate invalidate];
            }
            if (manager) {
                [manager stopUpdatingLocation];
                manager.delegate = nil;
            }
            delegate = nil;
            manager = nil;
            free(state);
        };

        if (wait_result != 0) {
            state->completed = YES;
            strlcpy(error_buffer, "Timed out waiting for macOS Location Services.", error_buffer_len);
            dispatch_async(dispatch_get_main_queue(), cleanup);
            return 2;
        }

        HoraryNativeLocationResult result = state->result;
        char native_error[512];
        strlcpy(native_error, state->error, sizeof(native_error));
        dispatch_async(dispatch_get_main_queue(), cleanup);

        if (native_error[0] != '\0') {
            strlcpy(error_buffer, native_error, error_buffer_len);
            return result.authorization_status == (int)kCLAuthorizationStatusDenied ? 3 : 1;
        }

        *out_result = result;
        return 0;
    }
}
