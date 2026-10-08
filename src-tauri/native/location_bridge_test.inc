// Debug-only test double for the production permission-refresh implementation.
// No OS consent or location service is invoked by these tests.
@interface HoraryLocationTestManager : NSObject
@property(nonatomic) CLLocationAccuracy desiredAccuracy;
@property(nonatomic) CLLocationDistance distanceFilter;
@property(nonatomic, weak) id<CLLocationManagerDelegate> delegate;
@property(nonatomic, strong) dispatch_queue_t callbackQueue;
@property(nonatomic) CLAuthorizationStatus initializedStatus;
@property(nonatomic) BOOL initialized;
@property(nonatomic) int authorizationRequests;
@property(nonatomic) int locationRequests;
@end

@implementation HoraryLocationTestManager
@synthesize delegate = _delegate;

- (CLAuthorizationStatus)authorizationStatus {
    return self.initialized ? self.initializedStatus : kCLAuthorizationStatusNotDetermined;
}

- (void)setDelegate:(id<CLLocationManagerDelegate>)delegate {
    _delegate = delegate;
    if (delegate) {
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 20 * NSEC_PER_MSEC), self.callbackQueue, ^{
            self.initialized = YES;
            [self.delegate locationManagerDidChangeAuthorization:(CLLocationManager *)self];
        });
    }
}

- (void)requestWhenInUseAuthorization { self.authorizationRequests += 1; }
- (void)startUpdatingLocation { self.locationRequests += 1; }
- (void)stopUpdatingLocation {}
@end

int horary_test_location_permission_refresh(int initialized_status) {
    @autoreleasepool {
        dispatch_queue_t queue = dispatch_queue_create("app.horary.location-test", DISPATCH_QUEUE_SERIAL);
        HoraryLocationTestManager *manager = [[HoraryLocationTestManager alloc] init];
        manager.callbackQueue = queue;
        manager.initializedStatus = (CLAuthorizationStatus)initialized_status;
        int status = horary_location_permission_status_on_queue(queue,
            ^{ return (CLLocationManager *)manager; });
        // Drain cleanup before inspecting the fake provider's effects.
        dispatch_sync(queue, ^{});
        return status | (manager.authorizationRequests << 8) | (manager.locationRequests << 16);
    }
}
