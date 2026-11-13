#import <CoreLocation/CoreLocation.h>

typedef void (*PoniLocationCallback)(void *context, double latitude, double longitude, int status, const char *place);

@interface PoniLocationDelegate : NSObject <CLLocationManagerDelegate>
@property(nonatomic, assign) PoniLocationCallback callback;
@property(nonatomic, assign) void *context;
@property(nonatomic, assign) BOOL waitingForAuthorization;
@property(nonatomic, assign) NSUInteger attempts;
@property(nonatomic, strong) CLGeocoder *geocoder;
- (void)finishWithStatus:(int)status latitude:(double)latitude longitude:(double)longitude place:(NSString *)place;
@end

@implementation PoniLocationDelegate

- (void)finishWithStatus:(int)status latitude:(double)latitude longitude:(double)longitude place:(NSString *)place {
    PoniLocationCallback callback = self.callback;
    self.callback = NULL;
    self.waitingForAuthorization = NO;
    if (callback) callback(self.context, latitude, longitude, status, place.UTF8String);
}

- (void)locationManagerDidChangeAuthorization:(CLLocationManager *)manager {
    if (!self.waitingForAuthorization) return;
    switch (manager.authorizationStatus) {
        case kCLAuthorizationStatusAuthorizedAlways:
            self.waitingForAuthorization = NO;
            [manager requestLocation];
            break;
        case kCLAuthorizationStatusDenied:
        case kCLAuthorizationStatusRestricted:
            [self finishWithStatus:1 latitude:0 longitude:0 place:nil];
            break;
        default:
            break;
    }
}

- (void)locationManager:(CLLocationManager *)manager didUpdateLocations:(NSArray<CLLocation *> *)locations {
    CLLocation *location = locations.lastObject;
    if (!location || !self.callback || self.geocoder) return;
    self.geocoder = [CLGeocoder new];
    PoniLocationDelegate *delegate = self;
    [self.geocoder reverseGeocodeLocation:location completionHandler:^(NSArray<CLPlacemark *> *placemarks, NSError *error) {
        if (delegate.callback) {
            CLPlacemark *placemark = placemarks.firstObject;
            NSString *place = placemark.locality ?: placemark.subAdministrativeArea ?: placemark.administrativeArea ?: placemark.country ?: @"当前位置";
            [delegate finishWithStatus:0 latitude:location.coordinate.latitude longitude:location.coordinate.longitude place:place];
        }
        delegate.geocoder = nil;
    }];
}

- (void)locationManager:(CLLocationManager *)manager didFailWithError:(NSError *)error {
    if (!self.callback || self.geocoder) return;
    BOOL coreLocationError = [error.domain isEqualToString:kCLErrorDomain];
    if (coreLocationError && error.code == kCLErrorLocationUnknown && self.attempts < 2) {
        // 单次定位可能在服务刚就绪时失败；仅重试一次，离开页面后不会重新启动。
        self.attempts += 1;
        __weak PoniLocationDelegate *weakDelegate = self;
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW, NSEC_PER_SEC), dispatch_get_main_queue(), ^{
            PoniLocationDelegate *delegate = weakDelegate;
            if (delegate.callback && !delegate.geocoder && manager.delegate == delegate) {
                [manager requestLocation];
            }
        });
        return;
    }
    int status = 2;
    if (coreLocationError && error.code == kCLErrorDenied) status = 1;
    if (coreLocationError && error.code == kCLErrorNetwork) status = 4;
    // 只传错误域和错误码，不把设备坐标或地名写入诊断日志。
    NSString *detail = [NSString stringWithFormat:@"%@ (%ld)", error.domain, (long)error.code];
    [self finishWithStatus:status latitude:0 longitude:0 place:detail];
}

@end

static CLLocationManager *poniManager;
static PoniLocationDelegate *poniDelegate;

int poni_location_needs_authorization(void) {
    return [CLLocationManager authorizationStatus] == kCLAuthorizationStatusNotDetermined;
}

void poni_location_request(PoniLocationCallback callback, void *context) {
    if (!poniManager) {
        poniDelegate = [PoniLocationDelegate new];
        poniManager = [CLLocationManager new];
        poniManager.delegate = poniDelegate;
        // 天气只需要城市级位置，无需等待百米级精度。
        poniManager.desiredAccuracy = kCLLocationAccuracyThreeKilometers;
    }
    poniDelegate.callback = callback;
    poniDelegate.context = context;
    poniDelegate.attempts = 1;
    poniDelegate.waitingForAuthorization = NO;
    if (![CLLocationManager locationServicesEnabled]) {
        [poniDelegate finishWithStatus:3 latitude:0 longitude:0 place:nil];
        return;
    }
    switch (poniManager.authorizationStatus) {
        case kCLAuthorizationStatusAuthorizedAlways:
            [poniManager requestLocation];
            break;
        case kCLAuthorizationStatusNotDetermined:
            poniDelegate.waitingForAuthorization = YES;
            // 授权完成后由 delegate 发起一次定位，避免同一时刻重复 requestLocation。
            [poniManager requestWhenInUseAuthorization];
            break;
        default:
            [poniDelegate finishWithStatus:1 latitude:0 longitude:0 place:nil];
            break;
    }
}

void poni_location_stop(void) {
    poniDelegate.callback = NULL;
    [poniDelegate.geocoder cancelGeocode];
    poniDelegate.geocoder = nil;
    poniManager.delegate = nil;
    [poniManager stopUpdatingLocation];
    poniManager = nil;
    poniDelegate = nil;
}
