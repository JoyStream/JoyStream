import { InMemoryCache } from '@apollo/client'
import { relayStylePagination } from '@apollo/client/utilities'
import { parseISO } from 'date-fns'

const cache = new InMemoryCache({
  typePolicies: {
    Query: {
      fields: {
        channelsConnection: relayStylePagination(),
        videosConnection: relayStylePagination((args) => {
          // make sure queries asking for a specific category are separated in cache
          return args?.where?.categoryId_eq
        }),
      },
    },
    Video: {
      fields: {
        publishedOnJoystreamAt: {
          merge(_, publishedOnJoystreamAt: string | Date): Date {
            if (typeof publishedOnJoystreamAt !== 'string') {
              // TODO: investigate further
              // rarely, for some reason the object that arrives here is already a date object
              // in this case parsing attempt will cause an error
              return publishedOnJoystreamAt
            }
            return parseISO(publishedOnJoystreamAt)
          },
        },
      },
    },
  },
  possibleTypes: {
    FreeTextSearchResultItemType: ['Video', 'Channel'],
  },
})

export default cache